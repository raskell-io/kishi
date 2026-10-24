//! The C ABI the web page calls, the same one-command-in, JSON-out protocol as
//! the chess and shogi engines:
//!
//!   position startpos [moves <D4|pass>...]   set the game
//!   state                                     board, legal points, captures, result
//!   go level <1-8> | go ms <milliseconds>     choose a move, "pass" or "resign" (does not play it)
//!   san <move>                                the move as written in the record (D4, pass)
//!   score                                     the final count: owner of every point, dead stones, margin

use crate::board::*;
use crate::mcts::{Decision, Limits, Searcher, final_score};
use std::cell::RefCell;

struct Web {
    input: Vec<u8>,
    output: String,
    searcher: Searcher,
    board: Board,
    /// Hashes of every position so far, for superko.
    history: Vec<u64>,
}

thread_local! {
    static WEB: RefCell<Web> = RefCell::new(Web {
        input: vec![],
        output: String::new(),
        searcher: Searcher::new(0x5eed_60_60),
        board: Board::new(),
        history: vec![Board::new().hash],
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
    WEB.with_borrow_mut(|w| w.searcher.reseed(seed.to_bits()));
}

/// Simulations per move at levels 1 to 6; levels 7 and 8 think for a fixed time.
fn limits(level: u32) -> Limits {
    let playouts = match level {
        0 | 1 => 300,
        2 => 800,
        3 => 2_000,
        4 => 5_000,
        5 => 10_000,
        6 => 20_000,
        _ => u32::MAX,
    };
    let ms = match level {
        7 => 1_500.0,
        8 => 5_000.0,
        _ => f64::INFINITY,
    };
    Limits { playouts, ms }
}

impl Web {
    fn run(&mut self, cmd: &str) -> String {
        let words: Vec<&str> = cmd.split_whitespace().collect();
        match words.first().copied() {
            Some("position") => match self.set_position(&words[1..]) {
                Ok(()) => self.state(),
                Err(e) => format!("{{\"error\":\"{e}\"}}"),
            },
            Some("state") => self.state(),
            Some("san") => match words.get(1).and_then(|m| parse(m)).filter(|&m| self.legal(m)) {
                Some(m) => format!("{{\"san\":\"{}\"}}", name(m)),
                None => "{\"error\":\"illegal move\"}".into(),
            },
            Some("go") => {
                let arg = words.get(2).and_then(|s| s.parse::<f64>().ok()).unwrap_or(8.0);
                let l = if words.get(1) == Some(&"ms") { Limits { playouts: u32::MAX, ms: arg } } else { limits(arg as u32) };
                self.go(l)
            }
            Some("score") => self.score(),
            Some("newgame") => "{}".into(),
            _ => format!("{{\"error\":\"unknown command: {}\"}}", cmd.replace('"', "'")),
        }
    }

    /// Legal for a player: anything the rules allow, own eyes included, but no repeated position.
    fn legal(&self, m: u16) -> bool {
        if m == PASS {
            return true;
        }
        if !self.board.is_legal(m, self.board.to_move) {
            return false;
        }
        let mut b = self.board.clone();
        b.play(m);
        !self.history.contains(&b.hash)
    }

    fn set_position(&mut self, words: &[&str]) -> Result<(), String> {
        let moves_at = words.iter().position(|w| *w == "moves").unwrap_or(words.len());
        self.board = Board::new();
        self.history = vec![self.board.hash];
        for w in words.iter().skip(moves_at + 1) {
            let m = parse(w).filter(|&m| self.legal(m)).ok_or(format!("illegal move {w}"))?;
            self.board.play(m);
            self.history.push(self.board.hash);
        }
        Ok(())
    }

    fn state(&self) -> String {
        let b = &self.board;
        let mut legal: Vec<String> = vec![];
        for y in 0..N {
            for x in 0..N {
                let p = point(x, y) as u16;
                if b.cells[p as usize] == EMPTY && self.legal(p) {
                    legal.push(format!("\"{}\"", name(p)));
                }
            }
        }
        legal.push("\"pass\"".into());
        let result = if b.passes >= 2 { "ended" } else { "" };
        format!(
            "{{\"board\":\"{}\",\"turn\":\"{}\",\"legal\":[{}],\"last\":\"{}\",\"ko\":\"{}\",\"captures\":{{\"b\":{},\"w\":{}}},\"komi\":{KOMI},\"check\":\"\",\"result\":\"{result}\"}}",
            b.picture(),
            if b.to_move == BLACK { 'b' } else { 'w' },
            legal.join(","),
            if b.moves == 0 { String::new() } else { name(b.last) },
            if b.ko == 0 { String::new() } else { name(b.ko) },
            b.captures[BLACK as usize],
            b.captures[WHITE as usize],
        )
    }

    fn go(&mut self, l: Limits) -> String {
        let t0 = kishi_engine::search::now_ms();
        let (board, history) = (self.board.clone(), self.history.clone());
        let (d, r) = self.searcher.decide(&board, &history, l);
        let mv = match d {
            Decision::Play(m) => name(m),
            Decision::Pass => "pass".into(),
            Decision::Resign => "resign".into(),
        };
        format!(
            "{{\"move\":\"{mv}\",\"san\":\"{mv}\",\"winrate\":{:.3},\"score\":{},\"mate\":null,\"depth\":0,\"nodes\":{},\"ms\":{:.0}}}",
            r.winrate,
            ((r.winrate - 0.5) * 200.0) as i32,
            r.playouts,
            kishi_engine::search::now_ms() - t0
        )
    }

    /// The count after two passes: who owns each point ('b', 'w' or '.'), which stones are
    /// dead ('1'), and black's margin after komi.
    fn score(&mut self) -> String {
        let board = self.board.clone();
        let own = self.searcher.ownership(&board, 600);
        let (dead, margin) = final_score(&board, &own);
        let (mut owner, mut gone) = (String::with_capacity(N * N), String::with_capacity(N * N));
        for y in 0..N {
            for x in 0..N {
                let p = point(x, y);
                owner.push(if own[p] > 0.4 { 'b' } else if own[p] < -0.4 { 'w' } else { '.' });
                gone.push(if dead[p] { '1' } else { '0' });
            }
        }
        format!(
            "{{\"owner\":\"{owner}\",\"dead\":\"{gone}\",\"margin\":{margin},\"winner\":\"{}\"}}",
            if margin > 0.0 { 'b' } else { 'w' }
        )
    }
}
