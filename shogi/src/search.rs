//! Iterative deepening principal variation search for shogi, on the same plan
//! as the chess engine: transposition table, null-move and reverse futility
//! pruning, late move reductions, killer and history ordering, a capture-only
//! quiescence search, and check extensions.

use crate::eval::{VALUE, evaluate};
use crate::position::*;
use kishi_engine::search::now_ms;
use kishi_engine::tt::*;
use kishi_engine::types::Move as Stored;

pub const MAX_PLY: usize = 96;
pub const INF: i32 = 32_001;
pub const MATE: i32 = 32_000;
pub const MATE_BOUND: i32 = MATE - MAX_PLY as i32;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub depth: i32,
    pub nodes: u64,
    pub soft_ms: f64,
    pub hard_ms: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { depth: MAX_PLY as i32 - 1, nodes: u64::MAX, soft_ms: f64::INFINITY, hard_ms: f64::INFINITY }
    }
}

impl Limits {
    pub fn depth(d: i32) -> Limits {
        Limits { depth: d, ..Default::default() }
    }
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub best: Move,
    pub score: i32,
    pub depth: i32,
    pub nodes: u64,
}

pub struct Searcher {
    tt: Tt,
    /// By side, origin (a square, or 81 + kind for a drop) and destination.
    history: Vec<[[i32; 81]; 89]>,
    killers: [[Move; 2]; MAX_PLY + 2],
    lists: Vec<Vec<Move>>,
    hashes: Vec<u64>,
    lmr: [[i32; 64]; 64],
    pub nodes: u64,
    stopped: bool,
    start: f64,
    limits: Limits,
    root_best: Move,
    root_score: i32,
}

#[inline]
fn to_tt(m: Move) -> Stored {
    Stored(m.0)
}
#[inline]
fn from_tt(m: Stored) -> Move {
    Move(m.0)
}

impl Searcher {
    pub fn new(tt_mb: usize) -> Searcher {
        let mut lmr = [[0; 64]; 64];
        for (d, row) in lmr.iter_mut().enumerate().skip(1) {
            for (m, r) in row.iter_mut().enumerate().skip(1) {
                *r = (0.7 + (d as f64).ln() * (m as f64).ln() / 2.2) as i32;
            }
        }
        Searcher {
            tt: Tt::new(tt_mb),
            history: vec![[[0; 81]; 89]; 2],
            killers: [[Move::NONE; 2]; MAX_PLY + 2],
            lists: (0..MAX_PLY + 2).map(|_| Vec::with_capacity(200)).collect(),
            hashes: Vec::with_capacity(1024),
            lmr,
            nodes: 0,
            stopped: false,
            start: 0.0,
            limits: Limits::default(),
            root_best: Move::NONE,
            root_score: 0,
        }
    }

    pub fn clear(&mut self) {
        self.tt.clear();
        for h in &mut self.history {
            *h = [[0; 81]; 89];
        }
    }

    fn begin(&mut self, pos: &Position, history: &[u64], limits: Limits) {
        self.start = now_ms();
        self.limits = limits;
        self.nodes = 0;
        self.stopped = false;
        self.tt.new_search();
        self.killers = [[Move::NONE; 2]; MAX_PLY + 2];
        self.hashes.clear();
        self.hashes.extend_from_slice(history);
        self.hashes.push(pos.hash);
    }

    /// Searches `pos`, reached through the positions hashed in `history`.
    pub fn search(&mut self, pos: &Position, history: &[u64], limits: Limits) -> SearchResult {
        self.begin(pos, history, limits);
        let legal = pos.legal_moves();
        let mut result = SearchResult { best: legal.first().copied().unwrap_or(Move::NONE), score: -MATE, depth: 0, nodes: 0 };
        if legal.len() <= 1 {
            return result;
        }
        let mut score = 0;
        for depth in 1..=limits.depth.max(1) {
            self.root_best = Move::NONE;
            let mut delta = 30;
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
            if self.root_best != Move::NONE && (!self.stopped || depth > 1) {
                result.best = self.root_best;
                result.score = if self.stopped { self.root_score } else { score };
                result.depth = if self.stopped { depth - 1 } else { depth };
            }
            if self.stopped || now_ms() - self.start >= limits.soft_ms {
                break;
            }
            if score.abs() >= MATE_BOUND && depth > (MATE - score.abs()) * 2 + 2 {
                break;
            }
        }
        result.nodes = self.nodes;
        result
    }

    /// Scores every legal root move with a full-window search to `depth`.
    pub fn score_root_moves(&mut self, pos: &Position, history: &[u64], depth: i32) -> Vec<(Move, i32)> {
        self.begin(pos, history, Limits::default());
        let mut scored = vec![];
        for m in pos.legal_moves() {
            let next = pos.make(m);
            self.hashes.push(next.hash);
            let s = -self.negamax(&next, -INF, INF, depth - 1, 1, false);
            self.hashes.pop();
            scored.push((m, s));
        }
        scored
    }

    #[inline]
    fn check_time(&mut self) {
        if self.nodes >= self.limits.nodes || (self.nodes & 1023 == 0 && now_ms() - self.start >= self.limits.hard_ms) {
            self.stopped = true;
        }
    }

    fn is_repetition(&self, pos: &Position) -> bool {
        let n = self.hashes.len();
        let mut i = n.saturating_sub(5);
        // Any earlier occurrence on the same side to move, within the last few dozen plies.
        let floor = n.saturating_sub(64);
        while i > floor {
            if self.hashes[i - 1] == pos.hash {
                return true;
            }
            i = i.saturating_sub(2);
        }
        false
    }

    fn order(&self, pos: &Position, ply: usize, list: &[Move], tt_move: Move) -> Vec<(Move, i32)> {
        list.iter()
            .map(|&m| {
                let s = if m == tt_move {
                    1 << 30
                } else if !m.is_drop() && pos.board[m.to()] != EMPTY {
                    let victim = VALUE[kind(pos.board[m.to()]) as usize];
                    let attacker = VALUE[kind(pos.board[m.from()]) as usize];
                    (1 << 28) + victim * 16 - attacker / 16 + if m.is_promotion() { 300 } else { 0 }
                } else if m.is_promotion() {
                    1 << 27
                } else if m == self.killers[ply][0] {
                    (1 << 26) + 1
                } else if m == self.killers[ply][1] {
                    1 << 26
                } else {
                    self.history[pos.stm][m.from()][m.to()]
                };
                (m, s)
            })
            .collect()
    }

    fn negamax(&mut self, pos: &Position, mut alpha: i32, beta: i32, mut depth: i32, ply: usize, was_null: bool) -> i32 {
        let pv = beta - alpha > 1;
        let in_check = pos.in_check();
        if in_check && ply < MAX_PLY / 2 {
            depth += 1;
        }
        if depth <= 0 {
            return self.qsearch(pos, alpha, beta, ply);
        }
        self.nodes += 1;
        self.check_time();
        if self.stopped {
            return 0;
        }
        if ply > 0 {
            if self.is_repetition(pos) {
                return 0;
            }
            if ply >= MAX_PLY {
                return evaluate(pos);
            }
        }

        let entry = self.tt.probe(pos.hash);
        let tt_move = entry.map_or(Move::NONE, |e| from_tt(e.mv));
        if let Some(e) = entry {
            let s = from_tt_score(e.score as i32, ply);
            if !pv && e.depth as i32 >= depth && ply > 0 {
                match e.bound() {
                    BOUND_EXACT => return s,
                    BOUND_LOWER if s >= beta => return s,
                    BOUND_UPPER if s <= alpha => return s,
                    _ => {}
                }
            }
        }

        let eval = if in_check { -INF } else { entry.map_or_else(|| evaluate(pos), |e| e.eval as i32) };
        if !pv && !in_check {
            if depth <= 6 && eval - 90 * depth >= beta && eval < MATE_BOUND {
                return eval;
            }
            if depth >= 3 && !was_null && eval >= beta {
                let r = 3 + depth / 4;
                let next = pos.make_null();
                self.hashes.push(next.hash);
                let s = -self.negamax(&next, -beta, -beta + 1, depth - r, ply + 1, true);
                self.hashes.pop();
                if self.stopped {
                    return 0;
                }
                if s >= beta {
                    return if s >= MATE_BOUND { beta } else { s };
                }
            }
        }

        let mut list = std::mem::take(&mut self.lists[ply]);
        list.clear();
        pos.generate(&mut list, false);
        let mut ordered = self.order(pos, ply, &list, tt_move);
        self.lists[ply] = list;

        let mut best = -INF;
        let mut best_move = Move::NONE;
        let mut legal = 0;
        let original_alpha = alpha;
        let mut quiets: Vec<Move> = Vec::new();

        for i in 0..ordered.len() {
            let mut bi = i;
            for j in i + 1..ordered.len() {
                if ordered[j].1 > ordered[bi].1 {
                    bi = j;
                }
            }
            ordered.swap(i, bi);
            let m = ordered[i].0;
            let capture = !m.is_drop() && pos.board[m.to()] != EMPTY;
            let quiet = !capture && !m.is_promotion();

            // Late move pruning of quiet moves in shallow, hopeless nodes.
            if ply > 0 && !in_check && quiet && best > -MATE_BOUND && depth <= 4 && legal as i32 > 6 + depth * depth * 3 {
                continue;
            }

            let next = pos.make(m);
            if !pos.is_legal(m, &next) {
                continue;
            }
            legal += 1;
            self.hashes.push(next.hash);
            let gives_check = next.in_check();
            let mut s;
            if legal == 1 {
                s = -self.negamax(&next, -beta, -alpha, depth - 1, ply + 1, false);
            } else {
                let mut r = 0;
                if depth >= 3 && quiet && !in_check && !gives_check {
                    r = self.lmr[depth.min(63) as usize][legal.min(63)];
                    r -= pv as i32;
                    r -= (m == self.killers[ply][0] || m == self.killers[ply][1]) as i32;
                    r -= self.history[pos.stm][m.from()][m.to()] / 8000;
                    r = r.clamp(0, depth - 2);
                }
                s = -self.negamax(&next, -alpha - 1, -alpha, depth - 1 - r, ply + 1, false);
                if s > alpha && r > 0 {
                    s = -self.negamax(&next, -alpha - 1, -alpha, depth - 1, ply + 1, false);
                }
                if s > alpha && s < beta {
                    s = -self.negamax(&next, -beta, -alpha, depth - 1, ply + 1, false);
                }
            }
            self.hashes.pop();
            if self.stopped {
                return 0;
            }
            if s > best {
                best = s;
                if s > alpha {
                    alpha = s;
                    best_move = m;
                    if ply == 0 {
                        self.root_best = m;
                        self.root_score = s;
                    }
                    if s >= beta {
                        if quiet {
                            if self.killers[ply][0] != m {
                                self.killers[ply][1] = self.killers[ply][0];
                                self.killers[ply][0] = m;
                            }
                            let bonus = (depth * depth * 16).min(1500);
                            let h = &mut self.history[pos.stm];
                            h[m.from()][m.to()] += bonus - h[m.from()][m.to()] * bonus / 16384;
                            for q in &quiets {
                                h[q.from()][q.to()] -= bonus + h[q.from()][q.to()] * bonus / 16384;
                            }
                        }
                        break;
                    }
                }
            }
            if quiet && quiets.len() < 48 {
                quiets.push(m);
            }
        }

        if legal == 0 {
            // No legal move is a loss in shogi, check or not.
            return -MATE + ply as i32;
        }
        let bound = if best >= beta {
            BOUND_LOWER
        } else if best > original_alpha {
            BOUND_EXACT
        } else {
            BOUND_UPPER
        };
        self.tt.store(pos.hash, to_tt(best_move), to_tt_score(best, ply), if in_check { 0 } else { eval }, depth, bound);
        best
    }

    fn qsearch(&mut self, pos: &Position, mut alpha: i32, beta: i32, ply: usize) -> i32 {
        self.nodes += 1;
        self.check_time();
        if self.stopped {
            return 0;
        }
        if ply >= MAX_PLY {
            return evaluate(pos);
        }
        let stand = evaluate(pos);
        if stand >= beta {
            return stand;
        }
        alpha = alpha.max(stand);
        let mut list = std::mem::take(&mut self.lists[ply]);
        list.clear();
        pos.generate(&mut list, true);
        let mut ordered: Vec<(Move, i32)> = list
            .iter()
            .map(|&m| (m, VALUE[kind(pos.board[m.to()]) as usize] * 16 - VALUE[kind(pos.board[m.from()]) as usize] / 16 + m.is_promotion() as i32 * 200))
            .collect();
        self.lists[ply] = list;
        ordered.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        let mut best = stand;
        for (m, _) in ordered {
            // Delta pruning: even this capture cannot reach alpha.
            if !m.is_promotion() && stand + VALUE[kind(pos.board[m.to()]) as usize] + 200 < alpha {
                continue;
            }
            let next = pos.make(m);
            if !pos.is_legal(m, &next) {
                continue;
            }
            let s = -self.qsearch(&next, -beta, -alpha, ply + 1);
            if self.stopped {
                return 0;
            }
            if s > best {
                best = s;
                if s > alpha {
                    alpha = s;
                    if s >= beta {
                        break;
                    }
                }
            }
        }
        best
    }
}

#[inline]
fn to_tt_score(s: i32, ply: usize) -> i32 {
    if s >= MATE_BOUND {
        s + ply as i32
    } else if s <= -MATE_BOUND {
        s - ply as i32
    } else {
        s
    }
}

#[inline]
fn from_tt_score(s: i32, ply: usize) -> i32 {
    if s >= MATE_BOUND {
        s - ply as i32
    } else if s <= -MATE_BOUND {
        s + ply as i32
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn best(sfen: &str, depth: i32) -> (String, i32) {
        let mut s = Searcher::new(16);
        let pos = Position::from_sfen(sfen).unwrap();
        let r = s.search(&pos, &[], Limits::depth(depth));
        (r.best.usi(), r.score)
    }

    #[test]
    fn mates_in_one() {
        // A gold drop on 1b mates the cornered king (the pawn could not: uchifuzume).
        let (m, s) = best("7lk/9/7G1/9/9/9/9/9/K8 b GP 1", 3);
        assert_eq!(m, "G*1b");
        assert_eq!(s, MATE - 1);
    }

    #[test]
    fn takes_a_free_rook() {
        let (m, _) = best("lnsgkgsnl/7b1/ppppppppp/9/4r4/9/PP1PPPPPP/1B5R1/LNSGKGSNL b - 1", 4);
        assert_eq!(m, "8h5e");
    }

    #[test]
    fn plays_from_the_start() {
        let mut s = Searcher::new(16);
        let r = s.search(&Position::startpos(), &[], Limits::depth(6));
        assert!(Position::startpos().parse_usi(&r.best.usi()).is_some());
        assert!(r.score.abs() < 300, "{}", r.score);
    }
}
