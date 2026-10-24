//! Plays Kishi's shogi engine against a USI engine (Fairy-Stockfish by default).
//!
//!   shogi-vs [--engine fairy-stockfish] [--skill 0] [--their-ms 200] [--ms 500] [--games 40] [--threads 6]
//!
//! Games come in pairs from the same random opening (four random plies), colours swapped.

use kishi_engine::types::Rng;
use kishi_shogi::position::{GOTE, SENTE};
use kishi_shogi::search::MATE_BOUND;
use kishi_shogi::*;
use kishi_tools::{arg, elo};
use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

struct Usi {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    go: String,
}

impl Usi {
    fn start(path: &str, skill: i32, ms: u64) -> Usi {
        let mut child = Command::new(path).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("start engine");
        let mut u = Usi { stdin: child.stdin.take().unwrap(), stdout: BufReader::new(child.stdout.take().unwrap()), go: format!("go movetime {ms}") };
        std::mem::forget(child);
        u.send("usi");
        u.wait("usiok");
        u.send("setoption name USI_Variant value shogi");
        u.send(&format!("setoption name Skill_Level value {skill}"));
        u.send("setoption name Threads value 1");
        u.send("setoption name USI_Hash value 16");
        u.send("isready");
        u.wait("readyok");
        u
    }
    fn send(&mut self, s: &str) {
        writeln!(self.stdin, "{s}").unwrap();
        self.stdin.flush().unwrap();
    }
    fn wait(&mut self, prefix: &str) -> String {
        let mut line = String::new();
        loop {
            line.clear();
            if self.stdout.read_line(&mut line).unwrap() == 0 {
                panic!("engine exited");
            }
            if line.starts_with(prefix) {
                return line.trim().to_string();
            }
        }
    }
    fn best(&mut self, start: &str, moves: &[String]) -> String {
        self.send(&format!("position sfen {start} moves {}", moves.join(" ")));
        let go = self.go.clone();
        self.send(&go);
        self.wait("bestmove").split_whitespace().nth(1).unwrap_or("resign").to_string()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let engine: String = arg(&args, "--engine", "fairy-stockfish".into());
    let skill: i32 = arg(&args, "--skill", 0);
    let their_ms: u64 = arg(&args, "--their-ms", 200);
    let ms: f64 = arg(&args, "--ms", 500.0);
    let pairs: u32 = arg::<u32>(&args, "--games", 40) / 2;
    let threads: usize = arg(&args, "--threads", 6);
    let next = AtomicU32::new(0);
    let score = Mutex::new((0u32, 0u32, 0u32));
    std::thread::scope(|s| {
        for _ in 0..threads {
            let (next, score, engine) = (&next, &score, &engine);
            s.spawn(move || {
                let mut kishi = Searcher::new(32);
                let mut other = Usi::start(engine, skill, their_ms);
                loop {
                    let pair = next.fetch_add(1, Ordering::Relaxed);
                    if pair >= pairs {
                        break;
                    }
                    let mut rng = Rng((pair as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
                    let mut start = Position::startpos();
                    for _ in 0..4 {
                        let m = start.legal_moves();
                        start = start.make(m[rng.below(m.len())]);
                    }
                    for kishi_sente in [true, false] {
                        let r = play(&mut kishi, &mut other, start, kishi_sente, ms);
                        let mut sc = score.lock().unwrap();
                        match r {
                            2 => sc.0 += 1,
                            1 => sc.1 += 1,
                            _ => sc.2 += 1,
                        }
                        let (w, d, l) = *sc;
                        let (e, err) = elo(w, d, l);
                        eprintln!("{} games  +{w} ={d} -{l}  elo {e:+.0} ± {err:.0}", w + d + l);
                    }
                }
                other.send("quit");
            });
        }
    });
    let (w, d, l) = *score.lock().unwrap();
    let (e, err) = elo(w, d, l);
    println!("Kishi shogi ({ms} ms) vs {engine} skill {skill} ({their_ms} ms): +{w} ={d} -{l}  elo {e:+.0} ± {err:.0}");
}

/// Kishi's result: 2 win, 1 draw, 0 loss.
fn play(kishi: &mut Searcher, other: &mut Usi, start: Position, kishi_sente: bool, ms: f64) -> u8 {
    kishi.clear();
    other.send("usinewgame");
    let start_sfen = start.sfen();
    let mut pos = start;
    let mut moves: Vec<String> = vec![];
    let mut seen: Vec<u64> = vec![];
    loop {
        let kishi_to_move = (pos.stm == SENTE) == kishi_sente;
        if pos.legal_moves().is_empty() {
            return if kishi_to_move { 0 } else { 2 };
        }
        if seen.iter().filter(|&&h| h == pos.hash).count() >= 3 || moves.len() >= 320 {
            return 1;
        }
        seen.push(pos.hash);
        let m = if kishi_to_move {
            let r = kishi.search(&pos, &seen[..seen.len() - 1], Limits { soft_ms: ms / 2.0, hard_ms: ms, ..Default::default() });
            if r.score <= -MATE_BOUND && r.score > -MATE_BOUND - 2 {
                // Mated next move whatever it plays; play on, the opponent must find it.
            }
            r.best
        } else {
            let u = other.best(&start_sfen, &moves);
            if u == "resign" || u == "win" {
                return if u == "resign" { 2 } else { 0 };
            }
            pos.parse_usi(&u).unwrap_or_else(|| panic!("opponent's {u} is illegal for Kishi in {} (side {})", pos.sfen(), if pos.stm == GOTE { "gote" } else { "sente" }))
        };
        moves.push(m.usi());
        pos = pos.make(m);
    }
}
