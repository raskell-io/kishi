//! The C ABI the web page calls, the same one-command-in, JSON-out protocol as
//! the chess engine:
//!
//!   position startpos|sfen <sfen> [moves <usi>...]   set the game
//!   state                                           sfen, legal moves, check, result
//!   go level <1-8> | go ms <milliseconds>           choose a move (does not play it)
//!   san <usi>                                       a move in Western notation (P-7f)

use crate::position::*;
use crate::search::{Limits, MATE, MATE_BOUND, Searcher};
use kishi_engine::types::Rng;
use std::cell::RefCell;

struct Web {
    input: Vec<u8>,
    output: String,
    searcher: Option<Searcher>,
    pos: Position,
    history: Vec<u64>,
    rng: Rng,
}

thread_local! {
    static WEB: RefCell<Web> = RefCell::new(Web {
        input: vec![],
        output: String::new(),
        searcher: None,
        pos: Position::startpos(),
        history: vec![],
        rng: Rng(0x5eed_5106_1d0c_a7e5),
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

#[unsafe(no_mangle)]
pub extern "C" fn kishi_seed(seed: f64) {
    WEB.with_borrow_mut(|w| w.rng = Rng(seed.to_bits() | 1));
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

enum Level {
    Noisy(i32, f64),
    Think(f64),
}

/// The same ladder as chess: root scoring with a softmax at 1 to 6, full strength above.
fn level(n: u32) -> Level {
    match n {
        0 | 1 => Level::Noisy(1, 300.0),
        2 => Level::Noisy(2, 170.0),
        3 => Level::Noisy(2, 90.0),
        4 => Level::Noisy(3, 50.0),
        5 => Level::Noisy(4, 25.0),
        6 => Level::Noisy(5, 12.0),
        7 => Level::Think(1000.0),
        _ => Level::Think(3000.0),
    }
}

/// Moves after which a game is called a draw (impasse is otherwise not judged).
const MOVE_LIMIT: usize = 400;

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
            Some("san") => match words.get(1).and_then(|u| self.pos.parse_usi(u)) {
                Some(m) => format!("{{\"san\":{}}}", quote(&self.pos.notation(m))),
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
            Some(&"sfen") => Position::from_sfen(&words[1..moves_at].join(" "))?,
            _ => Position::startpos(),
        };
        let mut history = vec![];
        for w in words.iter().skip(moves_at + 1) {
            let m = pos.parse_usi(w).ok_or(format!("illegal move {w}"))?;
            history.push(pos.hash);
            pos = pos.make(m);
        }
        self.pos = pos;
        self.history = history;
        Ok(())
    }

    fn result(&self, legal: usize) -> &'static str {
        if legal == 0 {
            "checkmate"
        } else if self.history.iter().filter(|&&h| h == self.pos.hash).count() >= 3 {
            "repetition"
        } else if self.history.len() >= MOVE_LIMIT {
            "moves"
        } else {
            ""
        }
    }

    fn state(&self) -> String {
        let legal = self.pos.legal_moves();
        let list: Vec<String> = legal.iter().map(|m| quote(&m.usi())).collect();
        let check = if self.pos.in_check() { square_name(self.pos.kings[self.pos.stm] as usize) } else { String::new() };
        format!(
            "{{\"sfen\":{},\"turn\":\"{}\",\"legal\":[{}],\"check\":{},\"result\":\"{}\"}}",
            quote(&self.pos.sfen()),
            if self.pos.stm == SENTE { 'b' } else { 'w' },
            list.join(","),
            quote(&check),
            self.result(legal.len())
        )
    }

    fn go(&mut self, lvl: Level) -> String {
        let (pos, history) = (self.pos, self.history.clone());
        let t0 = kishi_engine::search::now_ms();
        let (best, score, depth, nodes) = match lvl {
            Level::Think(ms) => {
                let r = self.searcher().search(&pos, &history, Limits { soft_ms: ms * 0.5, hard_ms: ms, ..Default::default() });
                (r.best, r.score, r.depth, r.nodes)
            }
            Level::Noisy(depth, temperature) => {
                let s = self.searcher();
                let scored = s.score_root_moves(&pos, &history, depth);
                let nodes = s.nodes;
                let top = scored.iter().map(|x| x.1).max().unwrap_or(0);
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
            quote(&best.usi()),
            quote(&pos.notation(best)),
            kishi_engine::search::now_ms() - t0
        )
    }
}
