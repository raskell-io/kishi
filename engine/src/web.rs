//! The C ABI the web page calls. One text command in, one JSON reply out:
//!
//!   kishi_input(len) -> pointer to write `len` bytes of command into
//!   kishi_command(len) -> length of the reply
//!   kishi_output() -> pointer to the reply
//!
//! Commands:
//!   position startpos|fen <fen> [moves <uci>...]   set the game
//!   state                                          fen, legal moves, check, result
//!   go level <1-8> | go ms <milliseconds>          choose a move (does not play it)
//!   san <uci>                                      a move in algebraic notation

use crate::position::Position;
use crate::search::{Limits, MATE, MATE_BOUND, Searcher};

#[cfg(feature = "threads")]
pub mod threads;
use crate::types::*;
use std::cell::RefCell;

struct Web {
    input: Vec<u8>,
    output: String,
    searcher: Option<Searcher>,
    pos: Position,
    history: Vec<u64>,
    /// The game's moves from the start position, when it began there.
    moves: Option<Vec<String>>,
    rng: Rng,
}

thread_local! {
    static WEB: RefCell<Web> = RefCell::new(Web {
        input: vec![],
        output: String::new(),
        searcher: None,
        pos: Position::startpos(),
        history: vec![],
        moves: Some(vec![]),
        rng: Rng(0x5eed_cafe_f00d_1234),
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn kishi_input(len: usize) -> *mut u8 {
    WEB.with_borrow_mut(|w| {
        w.input.resize(len, 0);
        w.input.as_mut_ptr()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn kishi_output() -> *const u8 {
    WEB.with_borrow(|w| w.output.as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn kishi_command(len: usize) -> usize {
    WEB.with_borrow_mut(|w| {
        let cmd = String::from_utf8_lossy(&w.input[..len]).into_owned();
        w.output = w.run(&cmd);
        w.output.len()
    })
}

/// Seeds the generator behind the deliberate mistakes at low levels.
#[unsafe(no_mangle)]
pub extern "C" fn kishi_seed(seed: f64) {
    WEB.with_borrow_mut(|w| w.rng = Rng(seed.to_bits() | 1));
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Main lines of the common openings, so the stronger levels do not play the
/// same game every time. Kishi follows a line while the game does.
const BOOK: &[&str] = &[
    "e2e4 e7e5 g1f3 b8c6 f1b5 a7a6 b5a4 g8f6 e1g1 f8e7",
    "e2e4 e7e5 g1f3 b8c6 f1b5 g8f6 e1g1 f6e4 d2d4",
    "e2e4 e7e5 g1f3 b8c6 f1c4 f8c5 c2c3 g8f6 d2d3",
    "e2e4 e7e5 g1f3 b8c6 f1c4 g8f6 d2d3 f8c5",
    "e2e4 e7e5 g1f3 b8c6 d2d4 e5d4 f3d4 g8f6 d4c6 b7c6",
    "e2e4 e7e5 g1f3 g8f6 f3e5 d7d6 e5f3 f6e4 d2d4",
    "e2e4 e7e5 b1c3 g8f6 g1f3 b8c6",
    "e2e4 c7c5 g1f3 d7d6 d2d4 c5d4 f3d4 g8f6 b1c3 a7a6",
    "e2e4 c7c5 g1f3 b8c6 d2d4 c5d4 f3d4 g8f6 b1c3 e7e5",
    "e2e4 c7c5 g1f3 e7e6 d2d4 c5d4 f3d4 b8c6",
    "e2e4 c7c5 b1c3 b8c6 g1f3 g7g6",
    "e2e4 c7c5 c2c3 g8f6 e4e5 f6d5 d2d4 c5d4",
    "e2e4 e7e6 d2d4 d7d5 b1c3 g8f6 c1g5 f8e7",
    "e2e4 e7e6 d2d4 d7d5 b1d2 c7c5 e4d5 e6d5",
    "e2e4 c7c6 d2d4 d7d5 b1c3 d5e4 c3e4 c8f5 e4g3 f5g6",
    "e2e4 c7c6 d2d4 d7d5 e4e5 c8f5 g1f3 e7e6",
    "e2e4 d7d6 d2d4 g8f6 b1c3 g7g6",
    "e2e4 g7g6 d2d4 f8g7 b1c3 d7d6",
    "e2e4 d7d5 e4d5 d8d5 b1c3 d5a5 d2d4 g8f6",
    "d2d4 d7d5 c2c4 e7e6 b1c3 g8f6 c1g5 f8e7 e2e3 e8g8",
    "d2d4 d7d5 c2c4 c7c6 g1f3 g8f6 b1c3 d5c4 a2a4",
    "d2d4 d7d5 c2c4 d5c4 g1f3 g8f6 e2e3 e7e6",
    "d2d4 d7d5 g1f3 g8f6 c1f4 e7e6 e2e3 c7c5",
    "d2d4 g8f6 c2c4 e7e6 b1c3 f8b4 e2e3 e8g8",
    "d2d4 g8f6 c2c4 e7e6 g1f3 b7b6 g2g3 c8b7",
    "d2d4 g8f6 c2c4 e7e6 g1f3 d7d5 b1c3 f8e7",
    "d2d4 g8f6 c2c4 g7g6 b1c3 f8g7 e2e4 d7d6 g1f3 e8g8",
    "d2d4 g8f6 c2c4 g7g6 b1c3 d7d5 c4d5 f6d5 e2e4 d5c3 b2c3",
    "d2d4 g8f6 c2c4 c7c5 d4d5 e7e6 b1c3 e6d5 c4d5 d7d6",
    "d2d4 g8f6 g1f3 g7g6 g2g3 f8g7 f1g2 e8g8",
    "d2d4 f7f5 g2g3 g8f6 f1g2 g7g6",
    "c2c4 e7e5 b1c3 g8f6 g1f3 b8c6 g2g3",
    "c2c4 g8f6 b1c3 e7e6 e2e4 d7d5",
    "c2c4 c7c5 g1f3 g8f6 b1c3 b8c6",
    "g1f3 d7d5 g2g3 g8f6 f1g2 c7c6 e1g1",
    "g1f3 g8f6 c2c4 g7g6 b1c3 f8g7",
];

/// How strong each level plays: (depth of the root scoring, softmax temperature
/// in centipawns) for the levels that make mistakes; a search budget for the rest.
#[derive(Clone, Copy)]
enum Level {
    Noisy(i32, f64),
    /// Full strength for a fixed time (`go ms`).
    Think(f64),
    /// Full strength for a fixed number of nodes, so the level plays the same on
    /// every device, under a time cap so a slow one never waits too long.
    Budget { nodes: u64, cap_ms: f64 },
}

fn level(n: u32) -> Level {
    match n {
        0 | 1 => Level::Noisy(1, 300.0),
        2 => Level::Noisy(2, 170.0),
        3 => Level::Noisy(3, 100.0),
        4 => Level::Noisy(4, 55.0),
        5 => Level::Noisy(5, 28.0),
        6 => Level::Noisy(7, 12.0),
        7 => Level::Budget { nodes: 1_200_000, cap_ms: 3_000.0 },
        _ => Level::Budget { nodes: 4_000_000, cap_ms: 6_000.0 },
    }
}

impl Web {
    fn searcher(&mut self) -> &mut Searcher {
        self.searcher.get_or_insert_with(|| Searcher::new(32))
    }

    fn run(&mut self, cmd: &str) -> String {
        let words: Vec<&str> = cmd.split_whitespace().collect();
        match words.first().copied() {
            Some("position") => match self.set_position(&words[1..]) {
                Ok(()) => self.state(),
                Err(e) => format!("{{\"error\":{}}}", quote(&e)),
            },
            Some("state") => self.state(),
            Some("san") => match words.get(1).and_then(|u| self.pos.parse_uci(u)) {
                Some(m) => format!("{{\"san\":{}}}", quote(&self.pos.san(m))),
                None => "{\"error\":\"illegal move\"}".into(),
            },
            Some("go") => {
                let arg = words.get(2).and_then(|s| s.parse::<f64>().ok()).unwrap_or(8.0);
                let lvl = if words.get(1) == Some(&"ms") { Level::Think(arg) } else { level(arg as u32) };
                self.go(lvl)
            }
            Some("newgame") => {
                self.searcher().clear();
                "{}".into()
            }
            _ => format!("{{\"error\":{}}}", quote(&format!("unknown command: {cmd}"))),
        }
    }

    fn set_position(&mut self, words: &[&str]) -> Result<(), String> {
        let moves_at = words.iter().position(|w| *w == "moves").unwrap_or(words.len());
        let mut pos = match words.first() {
            Some(&"fen") => Position::from_fen(&words[1..moves_at].join(" "))?,
            _ => Position::startpos(),
        };
        let mut history = vec![];
        let mut moves = vec![];
        for w in words.iter().skip(moves_at + 1) {
            let m = pos.parse_uci(w).ok_or(format!("illegal move {w}"))?;
            history.push(pos.hash);
            moves.push(m.uci());
            pos = pos.make(m);
        }
        self.pos = pos;
        self.history = history;
        self.moves = (words.first() != Some(&"fen")).then_some(moves);
        Ok(())
    }

    /// A random continuation of a book line the game has followed so far.
    fn book_move(&mut self) -> Option<Move> {
        let played = self.moves.as_ref()?;
        let options: Vec<&str> = BOOK
            .iter()
            .filter_map(|line| {
                let line: Vec<&str> = line.split_whitespace().collect();
                (line.len() > played.len() && line[..played.len()].iter().zip(played).all(|(a, b)| *a == b))
                    .then(|| line[played.len()])
            })
            .collect();
        if options.is_empty() {
            return None;
        }
        self.pos.parse_uci(options[self.rng.below(options.len())])
    }

    fn result(&self, legal: usize) -> &'static str {
        let p = &self.pos;
        if legal == 0 {
            if p.in_check() { "checkmate" } else { "stalemate" }
        } else if p.halfmove >= 100 {
            "fifty"
        } else if p.insufficient_material() {
            "material"
        } else if self.history.iter().filter(|&&h| h == p.hash).count() >= 2 {
            "repetition"
        } else {
            ""
        }
    }

    fn state(&self) -> String {
        let legal = self.pos.legal_moves();
        let list: Vec<String> = legal.iter().map(|m| quote(&m.uci())).collect();
        let check = if self.pos.in_check() { square_name(self.pos.king_square(self.pos.stm)) } else { String::new() };
        format!(
            "{{\"fen\":{},\"turn\":\"{}\",\"legal\":[{}],\"check\":{},\"result\":\"{}\"}}",
            quote(&self.pos.fen()),
            if self.pos.stm == WHITE { 'w' } else { 'b' },
            list.join(","),
            quote(&check),
            self.result(legal.len())
        )
    }

    fn go(&mut self, lvl: Level) -> String {
        let (pos, history) = (self.pos, self.history.clone());
        let t0 = crate::search::now_ms();
        if !matches!(lvl, Level::Noisy(_, t) if t > 60.0)
            && let Some(m) = self.book_move()
        {
            return format!(
                "{{\"move\":{},\"san\":{},\"score\":0,\"mate\":null,\"depth\":0,\"nodes\":0,\"ms\":0,\"book\":true}}",
                quote(&m.uci()),
                quote(&pos.san(m))
            );
        }
        let (best, score, depth, nodes) = match lvl {
            Level::Think(_) | Level::Budget { .. } => {
                let limits = match lvl {
                    Level::Think(ms) => Limits { soft_ms: ms * 0.5, hard_ms: ms, ..Default::default() },
                    // Stop starting iterations at the budget; stop outright at twice it or the cap.
                    Level::Budget { nodes, cap_ms } => Limits { soft_nodes: nodes, nodes: nodes * 2, hard_ms: cap_ms, ..Default::default() },
                    Level::Noisy(..) => unreachable!(),
                };
                #[cfg(feature = "threads")]
                threads::start(threads::Job { pos, history: history.clone(), shared: self.searcher().shared() });
                let r = self.searcher().search(&pos, &history, limits, &mut |_| {});
                #[cfg(feature = "threads")]
                threads::wait_idle();
                let best = r.best;
                (best, r.score, r.depth, r.nodes)
            }
            Level::Noisy(depth, temperature) => {
                let s = self.searcher();
                s.nodes = 0;
                let scored = s.score_root_moves(&pos, &history, depth);
                let nodes = s.nodes;
                let top = scored.iter().map(|x| x.1).max().unwrap_or(0);
                // Never walk into a mate it has seen, otherwise softmax over the scores.
                let weights: Vec<f64> = scored
                    .iter()
                    .map(|&(_, sc)| if sc <= -MATE_BOUND && top > -MATE_BOUND { 0.0 } else { (((sc - top) as f64) / temperature).exp() })
                    .collect();
                let total: f64 = weights.iter().sum();
                let mut x = self.rng.unit() * total;
                let mut pick = scored.first().map_or((Move::NONE, 0), |p| *p);
                for (i, w) in weights.iter().enumerate() {
                    if x < *w {
                        pick = scored[i];
                        break;
                    }
                    x -= w;
                }
                (pick.0, pick.1, depth, nodes)
            }
        };
        if best == Move::NONE {
            return "{\"error\":\"no legal moves\"}".into();
        }
        let mate = if score.abs() >= MATE_BOUND {
            let plies = MATE - score.abs();
            format!("{}", if score > 0 { (plies + 1) / 2 } else { -(plies + 1) / 2 })
        } else {
            "null".into()
        };
        format!(
            "{{\"move\":{},\"san\":{},\"score\":{score},\"mate\":{mate},\"depth\":{depth},\"nodes\":{nodes},\"ms\":{:.0}}}",
            quote(&best.uci()),
            quote(&pos.san(best)),
            crate::search::now_ms() - t0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn book_lines_are_legal() {
        for line in BOOK {
            let mut pos = Position::startpos();
            for m in line.split_whitespace() {
                pos = pos.make(pos.parse_uci(m).unwrap_or_else(|| panic!("{m} is illegal in {line}")));
            }
        }
    }
}
