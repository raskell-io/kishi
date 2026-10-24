//! Monte Carlo tree search with RAVE. Each simulation walks the tree to a leaf,
//! plays a quick random game to the end and scores it; the result counts for
//! every move on the way down, and (all moves as first) for every move either
//! side played later in that game, which is what makes search work on 19x19
//! without a trained network. Priors steer new nodes towards sensible shape.

use crate::board::*;
use kishi_engine::search::now_ms;
use kishi_engine::types::Rng;

#[derive(Clone, Copy, Default)]
struct Node {
    mv: u16,
    first_child: u32,
    n_children: u16,
    expanded: bool,
    visits: u32,
    /// Wins for the player who made `mv`.
    wins: f32,
    rave_visits: f32,
    rave_wins: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub playouts: u32,
    pub ms: f64,
}

pub struct Result {
    pub best: u16,
    /// Win rate of the best move, for the side that plays it.
    pub winrate: f32,
    pub playouts: u32,
}

/// RAVE equivalence parameter: how many visits before a move's own record outweighs its AMAF record.
const RAVE_EQUIV: f32 = 1800.0;
/// Prior evidence given to every new child, in virtual visits.
const PRIOR_VISITS: f32 = 24.0;
/// How many moves deep a random game may run before it is scored anyway.
const PLAYOUT_CAP: u16 = 3 * (N * N) as u16;

pub struct Searcher {
    nodes: Vec<Node>,
    rng: Rng,
    pub playouts: u32,
}

impl Searcher {
    pub fn new(seed: u64) -> Searcher {
        crate::patterns::init();
        Searcher { nodes: Vec::with_capacity(1 << 16), rng: Rng(seed | 1), playouts: 0 }
    }

    pub fn reseed(&mut self, seed: u64) {
        self.rng = Rng(seed | 1);
    }

    /// Prior win rate for a move, from simple shape: lines from the edge, fights near the last move.
    fn prior(board: &Board, m: u16) -> f32 {
        if m == PASS {
            // Passing is right once the board is settled; until then, almost never.
            return if board.moves > (N * N) as u16 { 0.3 } else { 0.02 };
        }
        let c = board.to_move;
        let p = m as usize;
        let (x, y) = xy(p);
        let line = x.min(y).min(N - 1 - x).min(N - 1 - y); // 0 = the edge
        let early = board.moves < 60;
        let mut v: f32 = match line {
            0 => 0.2,
            1 => if early { 0.35 } else { 0.45 },
            2 | 3 => if early { 0.62 } else { 0.5 },
            _ => if early { 0.5 } else { 0.5 },
        };
        // The opening is played in the corners, on the third and fourth lines.
        let corner = x.min(N - 1 - x) <= 4 && y.min(N - 1 - y) <= 4;
        if board.moves < 16 && corner && (2..=3).contains(&line) {
            v = 0.72;
        }
        // Taking a group in atari, or saving one of ours: the bigger, the more urgent.
        let taken = board.capture_size(p, c);
        if taken > 0 {
            v = v.max(if taken >= 3 { 0.9 } else { 0.75 });
        }
        let saved = board.rescue_size(p, c);
        if saved > 0 {
            v = v.max(if saved >= 3 { 0.88 } else { 0.72 });
        }
        if board.is_bad_self_atari(p, c) {
            v = 0.06;
        }
        if board.last != PASS {
            let (lx, ly) = xy(board.last as usize);
            let d = lx.abs_diff(x) + ly.abs_diff(y);
            if d <= 2 && v > 0.3 {
                v += 0.06;
            }
            // A good shape next to the last move.
            if lx.abs_diff(x) <= 1 && ly.abs_diff(y) <= 1 && crate::patterns::is_good(board.pattern_code(p)) && v > 0.3 {
                v = v.max(0.68);
            }
        }
        v
    }

    fn expand(&mut self, i: usize, board: &Board, legal: &[u16]) {
        let first = self.nodes.len() as u32;
        for &m in legal {
            let p = Self::prior(board, m);
            self.nodes.push(Node { mv: m, rave_visits: PRIOR_VISITS, rave_wins: PRIOR_VISITS * p, ..Default::default() });
        }
        let n = &mut self.nodes[i];
        n.first_child = first;
        n.n_children = legal.len() as u16;
        n.expanded = true;
    }

    fn select(&self, i: usize) -> usize {
        let n = &self.nodes[i];
        let mut best = n.first_child as usize;
        let mut best_v = f32::MIN;
        for c in n.first_child as usize..(n.first_child + n.n_children as u32) as usize {
            let ch = &self.nodes[c];
            let rv = ch.rave_wins / ch.rave_visits.max(1.0);
            let v = if ch.visits == 0 {
                rv
            } else {
                let q = ch.wins / ch.visits as f32;
                let nv = ch.visits as f32;
                let beta = (RAVE_EQUIV / (3.0 * nv + RAVE_EQUIV)).sqrt();
                (1.0 - beta) * q + beta * rv
            };
            if v > best_v {
                best_v = v;
                best = c;
            }
        }
        best
    }

    /// Searches `root`, whose game reached it through the positions hashed in `history`.
    pub fn search(&mut self, root: &Board, history: &[u64], limits: Limits) -> Result {
        let start = now_ms();
        self.nodes.clear();
        self.nodes.push(Node::default());
        self.playouts = 0;

        // At the root, also rule out moves that repeat an earlier position (positional superko).
        let legal: Vec<u16> = root
            .moves_for_search()
            .into_iter()
            .filter(|&m| {
                if m == PASS {
                    return true;
                }
                let mut b = root.clone();
                b.play(m);
                !history.contains(&b.hash)
            })
            .collect();
        if legal.len() == 1 {
            return Result { best: PASS, winrate: 0.5, playouts: 0 };
        }
        // The first moves go to the corners, as people play them: the search only chooses which.
        let opening = opening_points(root);
        let legal = if root.moves < 8 && !opening.is_empty() { opening.into_iter().filter(|m| legal.contains(m)).collect() } else { legal };
        self.expand(0, root, &legal);

        let mut path: Vec<usize> = Vec::with_capacity(256);
        let mut played = vec![0u8; CELLS]; // the colour that first played each point in this simulation
        loop {
            if self.playouts >= limits.playouts || (self.playouts & 63 == 0 && now_ms() - start >= limits.ms) {
                break;
            }
            let mut board = root.clone();
            path.clear();
            path.push(0);
            played.fill(0);
            let mut i = 0;
            // Down the tree.
            while self.nodes[i].expanded && self.nodes[i].n_children > 0 {
                let c = self.select(i);
                let m = self.nodes[c].mv;
                if m != PASS && played[m as usize] == 0 {
                    played[m as usize] = board.to_move;
                }
                board.play(m);
                path.push(c);
                i = c;
                if board.passes >= 2 {
                    break;
                }
            }
            // Grow the tree by one node once it has been visited.
            if !self.nodes[i].expanded && self.nodes[i].visits >= 1 && board.passes < 2 {
                let legal = board.moves_for_search();
                self.expand(i, &board, &legal);
                let c = self.select(i);
                let m = self.nodes[c].mv;
                if m != PASS && played[m as usize] == 0 {
                    played[m as usize] = board.to_move;
                }
                board.play(m);
                path.push(c);
            }
            // A quick random game to the end. Two passes in the tree only stop the tree walk.
            board.passes = 0;
            let mut n = 0;
            while board.passes < 2 && n < PLAYOUT_CAP {
                let m = board.playout_move(&mut self.rng);
                if m != PASS && played[m as usize] == 0 {
                    played[m as usize] = board.to_move;
                }
                board.play(m);
                n += 1;
            }
            let winner = if board.playout_score() > 0.0 { BLACK } else { WHITE };
            self.playouts += 1;

            // Back up: the node's own record, and AMAF for its siblings-to-be.
            let mut mover = root.to_move; // who made the move into path[1]
            for (depth, &node) in path.iter().enumerate() {
                if depth > 0 {
                    let nd = &mut self.nodes[node];
                    nd.visits += 1;
                    if mover == winner {
                        nd.wins += 1.0;
                    }
                    mover = opponent(mover);
                } else {
                    self.nodes[0].visits += 1;
                }
                // Children of this node are moves by `mover`; credit those it played later in the game.
                let nd = self.nodes[node];
                if nd.expanded {
                    for c in nd.first_child as usize..(nd.first_child + nd.n_children as u32) as usize {
                        let m = self.nodes[c].mv;
                        if m != PASS && played[m as usize] == mover {
                            let ch = &mut self.nodes[c];
                            ch.rave_visits += 1.0;
                            if mover == winner {
                                ch.rave_wins += 1.0;
                            }
                        }
                    }
                }
            }
        }

        let root_node = self.nodes[0];
        let mut best = root_node.first_child as usize;
        for c in root_node.first_child as usize..(root_node.first_child + root_node.n_children as u32) as usize {
            if self.nodes[c].visits > self.nodes[best].visits {
                best = c;
            }
        }
        let b = self.nodes[best];
        Result { best: b.mv, winrate: if b.visits > 0 { b.wins / b.visits as f32 } else { 0.5 }, playouts: self.playouts }
    }

    /// The root's most visited moves after a search: (move, visits, win rate, RAVE win rate).
    pub fn root_stats(&self, top: usize) -> Vec<(u16, u32, f32, f32)> {
        let r = self.nodes[0];
        let mut v: Vec<(u16, u32, f32, f32)> = (r.first_child as usize..(r.first_child + r.n_children as u32) as usize)
            .map(|c| {
                let n = self.nodes[c];
                (n.mv, n.visits, if n.visits > 0 { n.wins / n.visits as f32 } else { 0.0 }, n.rave_wins / n.rave_visits.max(1.0))
            })
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v.truncate(top);
        v
    }

    /// Who owns each point once the game is played out, averaged over random games: +1 black, -1 white.
    pub fn ownership(&mut self, root: &Board, games: u32) -> Vec<f32> {
        let mut own = vec![0f32; CELLS];
        for _ in 0..games {
            let mut b = root.clone();
            b.passes = 0;
            let mut n = 0;
            while b.passes < 2 && n < PLAYOUT_CAP {
                let m = b.playout_move(&mut self.rng);
                b.play(m);
                n += 1;
            }
            for y in 0..N {
                for x in 0..N {
                    let p = point(x, y);
                    let c = match b.cells[p] {
                        BLACK => BLACK,
                        WHITE => WHITE,
                        _ => {
                            // An empty point at the end is an eye: its neighbours say whose.
                            let up = b.cells[p - W];
                            let down = b.cells[p + W];
                            if up == BLACK || down == BLACK || b.cells[p - 1] == BLACK || b.cells[p + 1] == BLACK { BLACK } else { WHITE }
                        }
                    };
                    own[p] += if c == BLACK { 1.0 } else { -1.0 };
                }
            }
        }
        for v in &mut own {
            *v /= games as f32;
        }
        own
    }
}

pub enum Decision {
    Play(u16),
    Pass,
    Resign,
}

impl Searcher {
    /// What Kishi does with the move: search, then pass only when the count says it is
    /// winning (or nothing useful is left), and resign only when it is clearly lost.
    pub fn decide(&mut self, root: &Board, history: &[u64], limits: Limits) -> (Decision, Result) {
        let r = self.search(root, history, limits);
        let own = self.ownership(root, 160);
        let (_, margin) = final_score(root, &own);
        let mine = if root.to_move == BLACK { margin } else { -margin };
        let best_play = self
            .root_stats(usize::MAX)
            .into_iter()
            .find(|x| x.0 != PASS)
            .map(|x| x.0);
        let Some(play) = best_play else { return (Decision::Pass, r) };
        if (root.last == PASS && root.moves > 0 || r.best == PASS) && mine > 0.0 {
            return (Decision::Pass, r);
        }
        if r.winrate < 0.08 && root.moves > 120 && mine < -30.0 {
            return (Decision::Resign, r);
        }
        let mv = if r.best == PASS { play } else { r.best };
        (Decision::Play(mv), r)
    }
}

/// The 4-4, 3-4 and 3-3 points of every corner that is still empty.
fn opening_points(board: &Board) -> Vec<u16> {
    let mut out = vec![];
    for (cx, cy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        let flip = |v: usize, f: usize| if f == 1 { N - 1 - v } else { v };
        let empty = (0..6).all(|a| (0..6).all(|b| board.cells[point(flip(a, cx), flip(b, cy))] == EMPTY));
        if !empty {
            continue;
        }
        for (a, b) in [(3, 3), (2, 3), (3, 2), (2, 2)] {
            out.push(point(flip(a, cx), flip(b, cy)) as u16);
        }
    }
    out
}

/// The final count: stones the play-outs say are lost are taken off, then the
/// position is scored by area. Returns the dead stones and black's margin.
pub fn final_score(board: &Board, own: &[f32]) -> (Vec<bool>, f32) {
    let mut dead = vec![false; CELLS];
    for y in 0..N {
        for x in 0..N {
            let p = point(x, y);
            match board.cells[p] {
                BLACK if own[p] < -0.4 => dead[p] = true,
                WHITE if own[p] > 0.4 => dead[p] = true,
                _ => {}
            }
        }
    }
    let margin = board.area_score(&dead);
    (dead, margin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_away_from_the_edge() {
        let mut s = Searcher::new(3);
        let r = s.search(&Board::new(), &[], Limits { playouts: 3000, ms: f64::INFINITY });
        let (x, y) = xy(r.best as usize);
        let line = x.min(y).min(N - 1 - x).min(N - 1 - y);
        assert!(line >= 2, "opened on {}", name(r.best));
    }

    /// Black walls off the ten columns A to K, white the nine from L to T.
    fn split_board() -> Board {
        let mut b = Board::new();
        for row in 1..=N {
            b.play(parse(&format!("K{row}")).unwrap());
            b.play(parse(&format!("L{row}")).unwrap());
        }
        b
    }

    #[test]
    fn counts_a_finished_game() {
        let b = split_board();
        // 190 points against 171, less komi.
        assert_eq!(b.area_score(&[false; CELLS]), 190.0 - 171.0 - KOMI);
        let mut s = Searcher::new(4);
        let own = s.ownership(&b, 300);
        let (dead, margin) = final_score(&b, &own);
        assert!(dead.iter().all(|d| !d));
        assert_eq!(margin, 11.5);
    }

    #[test]
    fn passes_back_when_winning_and_plays_on_when_losing() {
        // Black plays inside its own area, white passes: black is ahead and ends the game.
        let mut b = split_board();
        b.play(parse("A1").unwrap());
        b.play(PASS);
        let mut s = Searcher::new(6);
        let (d, _) = s.decide(&b, &[], Limits { playouts: 2000, ms: f64::INFINITY });
        assert!(matches!(d, Decision::Pass), "black did not pass back while ahead");
        // Black passes instead: white is behind, so it plays on.
        let mut b = split_board();
        b.play(PASS);
        let (d, _) = s.decide(&b, &[], Limits { playouts: 2000, ms: f64::INFINITY });
        assert!(!matches!(d, Decision::Pass), "white passed while behind");
    }
}
