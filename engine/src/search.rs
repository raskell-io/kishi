//! Iterative deepening principal variation search with a quiescence search,
//! transposition table, null-move and futility pruning, late move reductions,
//! and killer, counter-move, history and continuation-history ordering.

use crate::attacks::*;
use crate::hce;
use crate::nnue::{Accumulator, Network};
use crate::position::Position;
use crate::tt::*;
use crate::types::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

pub const MAX_PLY: usize = 128;
pub const INF: i32 = 32_001;
pub const MATE: i32 = 32_000;
/// Scores beyond this are mates.
pub const MATE_BOUND: i32 = MATE - MAX_PLY as i32;

const SEE_VALUE: [i32; 7] = [100, 300, 300, 500, 900, 0, 0];
const CORRECTION_SIZE: usize = 16384;
/// Correction entries are kept in 1/256 centipawn.
const CORRECTION_GRAIN: i32 = 256;
const CORRECTION_MAX: i32 = 64 * CORRECTION_GRAIN;

pub fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        #[link(wasm_import_module = "env")]
        unsafe extern "C" {
            fn kishi_now() -> f64;
        }
        unsafe { kishi_now() }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::OnceLock;
        use std::time::Instant;
        static EPOCH: OnceLock<Instant> = OnceLock::new();
        EPOCH.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub depth: i32,
    /// Stop as soon as this many nodes are searched.
    pub nodes: u64,
    /// Do not start another iteration past this many nodes.
    pub soft_nodes: u64,
    /// Do not start another iteration past this time.
    pub soft_ms: f64,
    /// Stop as soon as this much time has passed.
    pub hard_ms: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { depth: MAX_PLY as i32 - 1, nodes: u64::MAX, soft_nodes: u64::MAX, soft_ms: f64::INFINITY, hard_ms: f64::INFINITY }
    }
}

impl Limits {
    pub fn depth(d: i32) -> Limits {
        Limits { depth: d, ..Default::default() }
    }
    pub fn movetime(ms: f64) -> Limits {
        Limits { soft_ms: ms, hard_ms: ms, ..Default::default() }
    }
    /// Budgets a move from the clock: time left, increment, moves to the next time control.
    pub fn clock(time_ms: f64, inc_ms: f64, moves_to_go: Option<u32>) -> Limits {
        let overhead = 30.0;
        let left = (time_ms - overhead).max(1.0);
        let mtg = moves_to_go.unwrap_or(25).clamp(1, 40) as f64;
        let soft = (left / mtg + inc_ms * 0.75).min(left * 0.5);
        let hard = (soft * 3.0).min(left * 0.8);
        Limits { soft_ms: soft * 0.6, hard_ms: hard, ..Default::default() }
    }
}

/// The parts of a searcher its helper threads share.
#[derive(Clone)]
pub struct Shared {
    pub tt: Arc<Tt>,
    pub stop: Arc<AtomicBool>,
    pub net: Option<&'static Network>,
}

pub struct Info<'a> {
    pub depth: i32,
    pub seldepth: usize,
    pub score: i32,
    pub nodes: u64,
    pub time_ms: f64,
    pub hashfull: usize,
    pub pv: &'a [Move],
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub best: Move,
    pub score: i32,
    pub depth: i32,
    pub nodes: u64,
    pub pv: Vec<Move>,
}

#[derive(Clone, Copy, Default)]
struct Frame {
    eval: i32,
    /// The move that led to this ply and the piece that made it.
    mv: Move,
    piece: u8,
    excluded: Move,
}

pub struct Searcher {
    /// Shared with helper threads, as is the stop flag.
    pub tt: Arc<Tt>,
    stop: Arc<AtomicBool>,
    /// Helpers search alongside a main thread, which alone keeps time and reports.
    helper: bool,
    net: Option<&'static Network>,
    acc: Vec<Accumulator>,
    history: Vec<[[i32; 64]; 64]>,
    capture_history: Vec<[[i32; 6]; 64]>,
    /// History of a move given the move one or two plies before it:
    /// [previous piece][previous to][piece][to].
    cont: Vec<i16>,
    /// How far the static evaluation has been off, by side to move and pawn structure.
    correction: Vec<[i32; CORRECTION_SIZE]>,
    counters: [[Move; 64]; 12],
    killers: [[Move; 2]; MAX_PLY + 2],
    frames: [Frame; MAX_PLY + 4],
    /// Hashes of the game so far, then the current search path.
    hashes: Vec<u64>,
    pv: Vec<[Move; MAX_PLY + 1]>,
    pv_len: [usize; MAX_PLY + 1],
    lmr: [[i32; 64]; 64],
    pub nodes: u64,
    seldepth: usize,
    stopped: bool,
    start: f64,
    limits: Limits,
    root_best: Move,
    root_score: i32,
}

impl Searcher {
    pub fn new(tt_mb: usize) -> Searcher {
        crate::attacks::init();
        let mut lmr = [[0; 64]; 64];
        for (d, row) in lmr.iter_mut().enumerate().skip(1) {
            for (m, r) in row.iter_mut().enumerate().skip(1) {
                *r = (0.8 + (d as f64).ln() * (m as f64).ln() / 2.3) as i32;
            }
        }
        Searcher {
            tt: Arc::new(Tt::new(tt_mb)),
            stop: Arc::new(AtomicBool::new(false)),
            helper: false,
            net: Network::embedded(),
            acc: vec![Accumulator::default(); MAX_PLY + 2],
            history: vec![[[0; 64]; 64]; 2],
            capture_history: vec![[[0; 6]; 64]; 12],
            cont: vec![0; 12 * 64 * 12 * 64],
            correction: vec![[0; CORRECTION_SIZE]; 2],
            counters: [[Move::NONE; 64]; 12],
            killers: [[Move::NONE; 2]; MAX_PLY + 2],
            frames: [Frame::default(); MAX_PLY + 4],
            hashes: Vec::with_capacity(1024),
            pv: vec![[Move::NONE; MAX_PLY + 1]; MAX_PLY + 1],
            pv_len: [0; MAX_PLY + 1],
            lmr,
            nodes: 0,
            seldepth: 0,
            stopped: false,
            start: 0.0,
            limits: Limits::default(),
            root_best: Move::NONE,
            root_score: 0,
        }
    }

    /// A searcher for another thread that shares this one's table, network and stop flag.
    pub fn helper(&self) -> Searcher {
        let mut h = Searcher::new(0);
        h.attach_to(self.shared());
        h
    }

    /// What helpers share with this searcher.
    pub fn shared(&self) -> Shared {
        Shared { tt: self.tt.clone(), stop: self.stop.clone(), net: self.net }
    }

    /// Makes this searcher a helper of the one that handed out `shared`.
    pub fn attach_to(&mut self, shared: Shared) {
        self.tt = shared.tt;
        self.stop = shared.stop;
        self.net = shared.net;
        self.helper = true;
    }

    /// Searches with `helpers` on other threads (Lazy SMP): they explore the
    /// same tree and share what they find through the transposition table.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn search_parallel(
        &mut self,
        helpers: &mut [Searcher],
        pos: &Position,
        history: &[u64],
        limits: Limits,
        report: &mut dyn FnMut(&Info),
    ) -> SearchResult {
        self.stop.store(false, Relaxed);
        std::thread::scope(|s| {
            for h in helpers.iter_mut() {
                s.spawn(move || h.search(pos, history, Limits::default(), &mut |_| {}));
            }
            self.search(pos, history, limits, report)
        })
    }

    /// Chooses the evaluation: a network, or None for the hand-written one.
    pub fn set_network(&mut self, net: Option<&'static Network>) {
        self.net = net;
    }

    pub fn network(&self) -> Option<&'static Network> {
        self.net
    }

    /// Forgets everything learned in earlier searches.
    pub fn clear(&mut self) {
        self.tt.clear();
        for h in &mut self.history {
            *h = [[0; 64]; 64];
        }
        for h in &mut self.capture_history {
            *h = [[0; 6]; 64];
        }
        self.cont.fill(0);
        for c in &mut self.correction {
            c.fill(0);
        }
        self.counters = [[Move::NONE; 64]; 12];
    }

    /// The static evaluation of a position, from the side to move's view.
    pub fn evaluate(&mut self, pos: &Position) -> i32 {
        match self.net {
            Some(net) => {
                net.refresh(&mut self.acc[0], pos);
                net.evaluate(&self.acc[0], pos)
            }
            None => hce::evaluate(pos),
        }
    }

    #[inline]
    fn eval_at(&self, pos: &Position, ply: usize) -> i32 {
        let raw = match self.net {
            Some(net) => net.evaluate(&self.acc[ply], pos),
            None => hce::evaluate(pos),
        };
        // Fade towards a draw as the fifty-move rule approaches.
        (raw * (200 - pos.halfmove as i32) / 200).clamp(-MATE_BOUND + 1, MATE_BOUND - 1)
    }

    /// Searches `pos`, reached through the positions hashed in `history`
    /// (oldest first, not including `pos`). `report` sees each finished iteration.
    pub fn search(&mut self, pos: &Position, history: &[u64], limits: Limits, report: &mut dyn FnMut(&Info)) -> SearchResult {
        self.start = now_ms();
        self.limits = limits;
        self.nodes = 0;
        self.stopped = false;
        if !self.helper {
            self.tt.new_search();
            self.stop.store(false, Relaxed);
        }
        self.killers = [[Move::NONE; 2]; MAX_PLY + 2];
        self.frames[0] = Frame::default();
        self.hashes.clear();
        self.hashes.extend_from_slice(history);
        self.hashes.push(pos.hash);
        if let Some(net) = self.net {
            net.refresh(&mut self.acc[0], pos);
        }

        let legal = pos.legal_moves();
        let mut result = SearchResult { best: legal.first().copied().unwrap_or(Move::NONE), score: 0, depth: 0, nodes: 0, pv: vec![] };
        if legal.is_empty() {
            result.score = if pos.in_check() { -MATE } else { 0 };
            return result;
        }

        let mut score = 0;
        for depth in 1..=limits.depth.max(1) {
            self.seldepth = 0;
            self.root_best = Move::NONE;
            // Aspiration windows around the last score, widening on failure.
            let mut delta = 20;
            let (mut alpha, mut beta) = if depth >= 5 { (score - delta, score + delta) } else { (-INF, INF) };
            loop {
                let s = self.negamax(pos, alpha, beta, depth, 0, false);
                if self.stopped {
                    break;
                }
                if s <= alpha {
                    beta = (alpha + beta) / 2;
                    alpha = (s - delta).max(-INF);
                } else if s >= beta {
                    beta = (s + delta).min(INF);
                } else {
                    score = s;
                    break;
                }
                delta += delta / 2;
            }
            // A partial iteration still improves on the last one if it found a move.
            if self.root_best != Move::NONE && (!self.stopped || depth > 1) {
                result.best = self.root_best;
                result.score = if self.stopped { self.root_score } else { score };
                result.depth = if self.stopped { depth - 1 } else { depth };
                result.pv = self.pv[0][..self.pv_len[0]].to_vec();
                if result.pv.first() != Some(&result.best) {
                    result.pv = vec![result.best];
                }
            }
            if self.stopped {
                break;
            }
            report(&Info {
                depth,
                seldepth: self.seldepth,
                score,
                nodes: self.nodes,
                time_ms: now_ms() - self.start,
                hashfull: self.tt.hashfull(),
                pv: &result.pv,
            });
            if self.nodes >= limits.soft_nodes || now_ms() - self.start >= limits.soft_ms {
                break;
            }
            if score.abs() >= MATE_BOUND && depth as i32 > (MATE - score.abs()) * 2 + 4 {
                break;
            }
        }
        result.nodes = self.nodes;
        if !self.helper {
            self.stop.store(true, Relaxed);
        }
        result
    }

    /// Scores every legal root move with a full-window search to `depth`;
    /// used to make deliberate, human-like mistakes at low levels.
    pub fn score_root_moves(&mut self, pos: &Position, history: &[u64], depth: i32) -> Vec<(Move, i32)> {
        self.start = now_ms();
        self.limits = Limits::default();
        self.stopped = false;
        self.hashes.clear();
        self.hashes.extend_from_slice(history);
        self.hashes.push(pos.hash);
        if let Some(net) = self.net {
            net.refresh(&mut self.acc[0], pos);
        }
        let mut scored = vec![];
        for m in pos.legal_moves() {
            let next = pos.make(m);
            self.push(pos, &next, m, 0);
            let s = if depth <= 1 { -self.qsearch(&next, -INF, INF, 1) } else { -self.negamax(&next, -INF, INF, depth - 1, 1, false) };
            self.hashes.pop();
            scored.push((m, s));
        }
        scored
    }

    #[inline]
    fn corrected(&self, pos: &Position, raw: i32) -> i32 {
        let c = self.correction[pos.stm][pos.pawn_hash as usize % CORRECTION_SIZE] / CORRECTION_GRAIN;
        (raw + c).clamp(-MATE_BOUND + 1, MATE_BOUND - 1)
    }

    /// Moves the correction for this pawn structure towards what the search found.
    fn update_correction(&mut self, pos: &Position, depth: i32, diff: i32) {
        let e = &mut self.correction[pos.stm][pos.pawn_hash as usize % CORRECTION_SIZE];
        let weight = (depth + 1).min(16);
        let target = diff * CORRECTION_GRAIN;
        *e = ((*e * (256 - weight) + target * weight) / 256).clamp(-CORRECTION_MAX, CORRECTION_MAX);
    }

    /// Where the continuation history for the move `back` plies before this
    /// node begins, if there was a real move there.
    #[inline]
    fn cont_base(&self, ply: usize, back: usize) -> Option<usize> {
        if ply < back {
            return None;
        }
        let f = self.frames[ply - back];
        (f.mv != Move::NONE && f.mv != Move::NULL && f.piece != NO_PIECE).then(|| (f.piece as usize * 64 + f.mv.to()) * 768)
    }

    #[inline]
    fn quiet_history(&self, pos: &Position, ply: usize, m: Move) -> i32 {
        let key = pos.board[m.from()] as usize * 64 + m.to();
        let mut h = self.history[pos.stm][m.from()][m.to()];
        for back in [0, 1] {
            if let Some(base) = self.cont_base(ply, back) {
                h += self.cont[base + key] as i32;
            }
        }
        h
    }

    #[inline]
    fn check_time(&mut self) {
        if self.nodes >= self.limits.nodes {
            self.stopped = true;
        }
        if self.nodes & 1023 == 0 {
            if self.stop.load(Relaxed) {
                self.stopped = true;
            } else if !self.helper && now_ms() - self.start >= self.limits.hard_ms {
                self.stopped = true;
                self.stop.store(true, Relaxed);
            }
        }
    }

    fn is_repetition(&self, pos: &Position) -> bool {
        let n = self.hashes.len();
        let reach = (pos.halfmove as usize).min(n - 1);
        let mut i = 2;
        while i <= reach {
            if self.hashes[n - 1 - i] == pos.hash {
                return true;
            }
            i += 2;
        }
        false
    }

    /// Records a move from `pos` to `next` at `ply`: path hash, accumulator, frame.
    #[inline]
    fn push(&mut self, pos: &Position, next: &Position, m: Move, ply: usize) {
        self.hashes.push(next.hash);
        if let Some(net) = self.net {
            let (head, tail) = self.acc.split_at_mut(ply + 1);
            net.update(&mut tail[0], &head[ply], pos, next);
        }
        self.frames[ply + 1].mv = m;
        self.frames[ply + 1].piece = pos.board[m.from()];
    }

    fn negamax(&mut self, pos: &Position, mut alpha: i32, mut beta: i32, mut depth: i32, ply: usize, cut_node: bool) -> i32 {
        let pv_node = beta - alpha > 1;
        let root = ply == 0;
        let in_check = pos.in_check();
        if in_check && ply < MAX_PLY / 2 {
            depth += 1;
        }
        self.pv_len[ply] = 0;
        if depth <= 0 {
            return self.qsearch(pos, alpha, beta, ply);
        }
        self.nodes += 1;
        self.check_time();
        if self.stopped {
            return 0;
        }
        self.seldepth = self.seldepth.max(ply);

        if !root {
            if pos.halfmove >= 100 || pos.insufficient_material() || self.is_repetition(pos) {
                return 0;
            }
            if ply >= MAX_PLY {
                return self.eval_at(pos, ply);
            }
            // Mate distance pruning.
            alpha = alpha.max(-MATE + ply as i32);
            beta = beta.min(MATE - ply as i32 - 1);
            if alpha >= beta {
                return alpha;
            }
        }

        let excluded = self.frames[ply].excluded;
        let entry = if excluded == Move::NONE { self.tt.probe(pos.hash) } else { None };
        let tt_move = entry.map_or(Move::NONE, |e| e.mv);
        if let Some(e) = entry {
            let s = score_from_tt(e.score as i32, ply);
            if !pv_node
                && e.depth as i32 >= depth
                && (e.bound() == BOUND_EXACT
                    || (e.bound() == BOUND_LOWER && s >= beta)
                    || (e.bound() == BOUND_UPPER && s <= alpha))
            {
                return s;
            }
        }

        let raw_eval = if in_check {
            -INF
        } else if let Some(e) = entry {
            e.eval as i32
        } else {
            self.eval_at(pos, ply)
        };
        let static_eval = if in_check { -INF } else { self.corrected(pos, raw_eval) };
        let eval = if in_check {
            -INF
        } else if let Some(e) = entry {
            let mut ev = static_eval;
            let s = score_from_tt(e.score as i32, ply);
            // The search score is a better estimate when its bound points the right way.
            if (e.bound() == BOUND_LOWER && s > ev) || (e.bound() == BOUND_UPPER && s < ev) || e.bound() == BOUND_EXACT {
                ev = s;
            }
            ev
        } else {
            static_eval
        };
        self.frames[ply].eval = eval;
        let improving = !in_check && ply >= 2 && eval > self.frames[ply - 2].eval;
        self.killers[ply + 1] = [Move::NONE; 2];

        if !pv_node && !in_check && excluded == Move::NONE {
            // Reverse futility: far enough above beta that a quiet move cannot fall below.
            if depth <= 8 && eval - 75 * (depth - improving as i32) >= beta && eval < MATE_BOUND {
                return (eval + beta) / 2;
            }
            // Razoring: hopeless unless a capture saves it.
            if depth <= 3 && eval + 250 * depth < alpha {
                let s = self.qsearch(pos, alpha, beta, ply);
                if s <= alpha {
                    return s;
                }
            }
            // Null move: if passing still beats beta, a real move will too.
            if depth >= 3
                && eval >= beta
                && self.frames[ply].mv != Move::NULL
                && pos.has_non_pawn_material(pos.stm)
                && beta > -MATE_BOUND
            {
                let r = 3 + depth / 3 + ((eval - beta) / 200).min(3);
                let next = pos.make_null();
                self.hashes.push(next.hash);
                if self.net.is_some() {
                    let (head, tail) = self.acc.split_at_mut(ply + 1);
                    tail[0].clone_from(&head[ply]);
                }
                self.frames[ply + 1].mv = Move::NULL;
                self.frames[ply + 1].piece = NO_PIECE;
                let s = -self.negamax(&next, -beta, -beta + 1, depth - r, ply + 1, !cut_node);
                self.hashes.pop();
                if self.stopped {
                    return 0;
                }
                if s >= beta {
                    return if s >= MATE_BOUND { beta } else { s };
                }
            }
        }

        // Without a hash move, a shallower search is as good and cheaper.
        if depth >= 4 && tt_move == Move::NONE && (pv_node || cut_node) && excluded == Move::NONE {
            depth -= 1;
        }

        let mut list = MoveList::new();
        pos.generate(&mut list, false);
        let mut scores = [0i32; 256];
        let prev = self.frames[ply];
        let counter = if prev.piece != NO_PIECE && prev.mv != Move::NULL && prev.mv != Move::NONE {
            self.counters[prev.piece as usize][prev.mv.to()]
        } else {
            Move::NONE
        };
        for i in 0..list.len {
            let m = list.moves[i];
            scores[i] = if m == tt_move {
                1 << 30
            } else if !m.is_quiet() {
                let victim = kind_of(pos.captured(m).min(11));
                let victim_value = if m.is_capture() { SEE_VALUE[victim] } else { 0 };
                let promo = if m.is_promotion() && m.promotion() == QUEEN { 900 } else { 0 };
                let hist = if m.is_capture() { self.capture_history[pos.board[m.from()] as usize][m.to()][victim] } else { 0 };
                let base = victim_value * 16 + promo * 16 + hist / 16;
                if see(pos, m, 0) { (1 << 28) + base } else { -(1 << 20) + base }
            } else if m == self.killers[ply][0] {
                (1 << 27) + 2
            } else if m == self.killers[ply][1] {
                (1 << 27) + 1
            } else if m == counter {
                1 << 27
            } else {
                self.quiet_history(pos, ply, m)
            };
        }

        let mut best_score = -INF;
        let mut best_move = Move::NONE;
        let mut legal = 0;
        let mut quiets_tried: [Move; 64] = [Move::NONE; 64];
        let mut n_quiets = 0;
        let mut captures_tried: [Move; 32] = [Move::NONE; 32];
        let mut n_captures = 0;
        let original_alpha = alpha;
        let mut skip_quiets = false;

        for i in 0..list.len {
            // Selection sort: bring the best remaining move forward.
            let mut bi = i;
            for j in i + 1..list.len {
                if scores[j] > scores[bi] {
                    bi = j;
                }
            }
            list.moves.swap(i, bi);
            scores.swap(i, bi);
            let m = list.moves[i];
            if m == excluded {
                continue;
            }
            let quiet = m.is_quiet();
            if quiet && skip_quiets {
                continue;
            }

            if !root && best_score > -MATE_BOUND && pos.has_non_pawn_material(pos.stm) {
                let lmr_depth = (depth - self.lmr[depth.min(63) as usize][(legal + 1).min(63)]).max(0);
                if quiet {
                    // Late move pruning.
                    if legal >= (3 + depth * depth) as usize / (2 - improving as usize) {
                        skip_quiets = true;
                        continue;
                    }
                    // Futility pruning.
                    if !in_check && lmr_depth <= 8 && eval + 100 + 90 * lmr_depth <= alpha {
                        skip_quiets = true;
                        continue;
                    }
                    if lmr_depth <= 6 && !see(pos, m, -60 * lmr_depth) {
                        continue;
                    }
                } else if depth <= 8 && !see(pos, m, -25 * depth * depth) {
                    continue;
                }
            }

            let next = pos.make(m);
            if next.left_king_in_check() {
                continue;
            }
            legal += 1;

            // Singular extension: if the hash move is much better than every
            // alternative, search it deeper.
            let mut extension = 0;
            if !root
                && depth >= 8
                && m == tt_move
                && excluded == Move::NONE
                && ply < MAX_PLY / 2
                && let Some(e) = entry
                && e.depth as i32 >= depth - 3
                && e.bound() != BOUND_UPPER
                && (e.score as i32).abs() < MATE_BOUND
            {
                let s_beta = e.score as i32 - depth * 2;
                self.frames[ply].excluded = m;
                let s = self.negamax(pos, s_beta - 1, s_beta, (depth - 1) / 2, ply, cut_node);
                self.frames[ply].excluded = Move::NONE;
                if self.stopped {
                    return 0;
                }
                if s < s_beta {
                    extension = 1;
                } else if s_beta >= beta {
                    // Multi-cut: more than one move beats beta.
                    return s_beta;
                }
            }

            self.push(pos, &next, m, ply);
            let new_depth = depth - 1 + extension;
            let gives_check = next.in_check();

            let mut s;
            if legal == 1 {
                s = -self.negamax(&next, -beta, -alpha, new_depth, ply + 1, !pv_node && !cut_node);
            } else {
                // Late move reductions, then a null-window search, re-searched as needed.
                let mut r = 0;
                if depth >= 3 && legal > 1 + pv_node as usize {
                    if quiet {
                        r = self.lmr[depth.min(63) as usize][legal.min(63)];
                        r += !improving as i32;
                        r += cut_node as i32;
                        r -= pv_node as i32;
                        r -= gives_check as i32;
                        r -= (m == self.killers[ply][0] || m == self.killers[ply][1] || m == counter) as i32;
                        r -= self.quiet_history(pos, ply, m) / 10000;
                    } else if scores[i] < 0 {
                        r = 1 + cut_node as i32;
                    }
                    r = r.clamp(0, new_depth - 1);
                }
                s = -self.negamax(&next, -alpha - 1, -alpha, new_depth - r, ply + 1, true);
                if s > alpha && r > 0 {
                    s = -self.negamax(&next, -alpha - 1, -alpha, new_depth, ply + 1, !cut_node);
                }
                if s > alpha && s < beta {
                    s = -self.negamax(&next, -beta, -alpha, new_depth, ply + 1, false);
                }
            }
            self.hashes.pop();
            if self.stopped {
                return 0;
            }

            if s > best_score {
                best_score = s;
                if s > alpha {
                    best_move = m;
                    alpha = s;
                    if pv_node {
                        self.pv[ply][0] = m;
                        let len = self.pv_len[ply + 1].min(MAX_PLY - 1);
                        let (head, tail) = self.pv.split_at_mut(ply + 1);
                        head[ply][1..=len].copy_from_slice(&tail[0][..len]);
                        self.pv_len[ply] = len + 1;
                    }
                    if root {
                        self.root_best = m;
                        self.root_score = s;
                    }
                    if s >= beta {
                        self.reward(pos, ply, depth, m, &quiets_tried[..n_quiets], &captures_tried[..n_captures]);
                        break;
                    }
                }
            }
            if quiet && n_quiets < 64 {
                quiets_tried[n_quiets] = m;
                n_quiets += 1;
            } else if m.is_capture() && n_captures < 32 {
                captures_tried[n_captures] = m;
                n_captures += 1;
            }
        }

        if legal == 0 {
            return if excluded != Move::NONE {
                alpha
            } else if in_check {
                -MATE + ply as i32
            } else {
                0
            };
        }

        if excluded == Move::NONE {
            let bound = if best_score >= beta {
                BOUND_LOWER
            } else if best_score > original_alpha {
                BOUND_EXACT
            } else {
                BOUND_UPPER
            };
            let eval_store = if in_check { 0 } else { raw_eval };
            self.tt.store(pos.hash, best_move, score_to_tt(best_score, ply), eval_store, depth, bound);
            if !in_check
                && (best_move == Move::NONE || best_move.is_quiet())
                && !(bound == BOUND_LOWER && best_score <= static_eval)
                && !(bound == BOUND_UPPER && best_score >= static_eval)
                && best_score.abs() < MATE_BOUND
            {
                self.update_correction(pos, depth, best_score - static_eval);
            }
        }
        best_score
    }

    /// Credits the move that caused a cutoff and debits the ones tried before it.
    fn reward(&mut self, pos: &Position, ply: usize, depth: i32, m: Move, quiets: &[Move], captures: &[Move]) {
        let bonus = (16 * depth * depth + 32 * depth).min(1600);
        let stm = pos.stm;
        let gravity = |h: &mut i32, b: i32| *h += b - *h * b.abs() / 16384;
        if m.is_quiet() {
            if self.killers[ply][0] != m {
                self.killers[ply][1] = self.killers[ply][0];
                self.killers[ply][0] = m;
            }
            let prev = self.frames[ply];
            if prev.piece != NO_PIECE && prev.mv != Move::NULL && prev.mv != Move::NONE {
                self.counters[prev.piece as usize][prev.mv.to()] = m;
            }
            gravity(&mut self.history[stm][m.from()][m.to()], bonus);
            for &q in quiets {
                gravity(&mut self.history[stm][q.from()][q.to()], -bonus);
            }
            for back in [0, 1] {
                if let Some(base) = self.cont_base(ply, back) {
                    let mut update = |mv: Move, b: i32| {
                        let e = &mut self.cont[base + pos.board[mv.from()] as usize * 64 + mv.to()];
                        let mut h = *e as i32;
                        gravity(&mut h, b);
                        *e = h as i16;
                    };
                    update(m, bonus);
                    for &q in quiets {
                        update(q, -bonus);
                    }
                }
            }
        } else if m.is_capture() {
            let victim = kind_of(pos.captured(m));
            gravity(&mut self.capture_history[pos.board[m.from()] as usize][m.to()][victim], bonus);
        }
        for &c in captures {
            let victim = kind_of(pos.captured(c));
            gravity(&mut self.capture_history[pos.board[c.from()] as usize][c.to()][victim], -bonus);
        }
    }

    fn qsearch(&mut self, pos: &Position, mut alpha: i32, beta: i32, ply: usize) -> i32 {
        self.nodes += 1;
        self.check_time();
        if self.stopped {
            return 0;
        }
        self.seldepth = self.seldepth.max(ply);
        if ply >= MAX_PLY {
            return self.eval_at(pos, ply);
        }
        if pos.halfmove >= 100 || pos.insufficient_material() {
            return 0;
        }
        let pv_node = beta - alpha > 1;
        let entry = self.tt.probe(pos.hash);
        if let Some(e) = entry {
            let s = score_from_tt(e.score as i32, ply);
            if !pv_node
                && (e.bound() == BOUND_EXACT
                    || (e.bound() == BOUND_LOWER && s >= beta)
                    || (e.bound() == BOUND_UPPER && s <= alpha))
            {
                return s;
            }
        }

        let in_check = pos.in_check();
        let mut best = -INF;
        let mut eval = -INF;
        let mut raw_eval = 0;
        if !in_check {
            raw_eval = match entry {
                Some(e) => e.eval as i32,
                None => self.eval_at(pos, ply),
            };
            eval = self.corrected(pos, raw_eval);
            best = eval;
            if best >= beta {
                return best;
            }
            alpha = alpha.max(best);
        }

        let mut list = MoveList::new();
        pos.generate(&mut list, !in_check);
        let mut scores = [0i32; 256];
        for i in 0..list.len {
            let m = list.moves[i];
            let victim = if m.is_capture() { SEE_VALUE[kind_of(pos.captured(m))] } else { 0 };
            let promo = if m.is_promotion() { 800 } else { 0 };
            scores[i] = if !in_check || m.is_capture() {
                victim * 16 + promo - kind_of(pos.board[m.from()]) as i32
            } else {
                -(1 << 20) + self.history[pos.stm][m.from()][m.to()]
            };
        }

        let mut best_move = Move::NONE;
        let mut legal = 0;
        for i in 0..list.len {
            let mut bi = i;
            for j in i + 1..list.len {
                if scores[j] > scores[bi] {
                    bi = j;
                }
            }
            list.moves.swap(i, bi);
            scores.swap(i, bi);
            let m = list.moves[i];

            if !in_check {
                // Delta pruning: even winning this piece outright cannot reach alpha.
                if !m.is_promotion() && eval + 150 + SEE_VALUE[kind_of(pos.captured(m))] <= alpha {
                    continue;
                }
                if !see(pos, m, 0) {
                    continue;
                }
            }
            let next = pos.make(m);
            if next.left_king_in_check() {
                continue;
            }
            legal += 1;
            self.push(pos, &next, m, ply);
            let s = -self.qsearch(&next, -beta, -alpha, ply + 1);
            self.hashes.pop();
            if self.stopped {
                return 0;
            }
            if s > best {
                best = s;
                if s > alpha {
                    best_move = m;
                    alpha = s;
                    if s >= beta {
                        break;
                    }
                }
            }
        }
        if in_check && legal == 0 {
            return -MATE + ply as i32;
        }
        let bound = if best >= beta { BOUND_LOWER } else { BOUND_UPPER };
        let eval_store = raw_eval;
        self.tt.store(pos.hash, best_move, score_to_tt(best, ply), eval_store, 0, bound);
        best
    }
}

/// Mate scores in the table count from the node, not from the root.
#[inline]
fn score_to_tt(s: i32, ply: usize) -> i32 {
    if s >= MATE_BOUND {
        s + ply as i32
    } else if s <= -MATE_BOUND {
        s - ply as i32
    } else {
        s
    }
}

#[inline]
fn score_from_tt(s: i32, ply: usize) -> i32 {
    if s >= MATE_BOUND {
        s - ply as i32
    } else if s <= -MATE_BOUND {
        s + ply as i32
    } else {
        s
    }
}

/// Static exchange evaluation: does `m` win at least `threshold` once every
/// capture on its square has been played out, cheapest attacker first?
pub fn see(pos: &Position, m: Move, threshold: i32) -> bool {
    if m.is_castle() || m.flag() == EN_PASSANT || m.is_promotion() {
        return threshold <= 0;
    }
    let (from, to) = (m.from(), m.to());
    let target = pos.board[to];
    let mut swap = if target == NO_PIECE { 0 } else { SEE_VALUE[kind_of(target)] } - threshold;
    if swap < 0 {
        return false;
    }
    swap = SEE_VALUE[kind_of(pos.board[from])] - swap;
    if swap <= 0 {
        return true;
    }
    let mut occ = pos.occupied() ^ bit(from) ^ bit(to);
    let mut stm = pos.stm;
    let mut attackers = pos.attackers_to(to, occ);
    let diagonal = pos.by_kind[BISHOP] | pos.by_kind[QUEEN];
    let straight = pos.by_kind[ROOK] | pos.by_kind[QUEEN];
    let mut res = 1;
    loop {
        stm ^= 1;
        attackers &= occ;
        let mine = attackers & pos.by_color[stm];
        if mine == 0 {
            break;
        }
        res ^= 1;
        let kind = (PAWN..=KING).find(|&k| mine & pos.by_kind[k] != 0).unwrap();
        if kind == KING {
            return if attackers & pos.by_color[stm ^ 1] != 0 { res ^ 1 != 0 } else { res != 0 };
        }
        swap = SEE_VALUE[kind] - swap;
        if swap < res {
            break;
        }
        occ ^= bit(lsb(mine & pos.by_kind[kind]));
        if kind == PAWN || kind == BISHOP || kind == QUEEN {
            attackers |= bishop_attacks(to, occ) & diagonal;
        }
        if kind == ROOK || kind == QUEEN {
            attackers |= rook_attacks(to, occ) & straight;
        }
    }
    res != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn best(fen: &str, depth: i32) -> (String, i32) {
        let mut s = Searcher::new(16);
        let pos = Position::from_fen(fen).unwrap();
        let r = s.search(&pos, &[], Limits::depth(depth), &mut |_| {});
        (r.best.uci(), r.score)
    }

    #[test]
    fn finds_mates() {
        // A back-rank mate, then a mate in two, then the scholar's mate.
        let (m, s) = best("6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1", 6);
        assert_eq!(m, "d1d8");
        assert_eq!(s, MATE - 1);
        let (_, s) = best("r2qkb1r/pp2nppp/3p4/2pNN1B1/2BnP3/3P4/PPP2PPP/R2bK2R w KQkq - 1 1", 6);
        assert!(s >= MATE - 3, "{s}");
        let (_, s) = best("r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 4 4", 4);
        assert_eq!(s, MATE - 1);
    }

    #[test]
    fn wins_material() {
        // A free queen.
        let (m, _) = best("rnb1kbnr/pppp1ppp/8/4p1q1/3P4/2N5/PPP1PPPP/R1BQKBNR w KQkq - 0 1", 5);
        assert_eq!(m, "c1g5");
    }

    #[test]
    fn stalemate_and_mate_scores() {
        let (_, s) = best("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1", 3);
        assert_eq!(s, 0);
    }

    #[test]
    fn see_values() {
        let pos = Position::from_fen("1k1r4/1pp4p/p7/4p3/8/P5P1/1PP4P/2K1R3 w - - 0 1").unwrap();
        assert!(see(&pos, pos.parse_uci("e1e5").unwrap(), 0));
        let pos = Position::from_fen("1k1r3q/1ppn3p/p4b2/4p3/8/P2N2P1/1PP1R1BP/2K1Q3 w - - 0 1").unwrap();
        assert!(!see(&pos, pos.parse_uci("d3e5").unwrap(), 0));
    }
}
