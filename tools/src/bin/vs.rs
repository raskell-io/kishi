//! Plays Kishi against an external UCI engine, to place it on a known scale.
//!
//!   vs --brain nets/kishi.nnue --engine stockfish --lichess 5 [--games 100] [--ms 1000]
//!   vs --brain nets/kishi.nnue --engine stockfish --elo 2400
//!
//! `--lichess N` sets the opponent up like lichess's computer at level N
//! (Stockfish skill level, depth limit and think time, as fishnet plays it).
//! `--elo E` uses Stockfish's own UCI_Elo strength limit instead.

use kishi_engine::search::MATE_BOUND;
use kishi_engine::types::*;
use kishi_engine::*;
use kishi_tools::*;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

/// lichess AI levels as fishnet plays them: Stockfish skill level, depth, milliseconds.
/// Stockfish accepts skill 0..20; lichess's lowest levels go below 0, so these are stronger.
const LICHESS: [(i32, u32, u32); 8] = [(-9, 5, 50), (-5, 5, 100), (-1, 5, 150), (3, 5, 200), (7, 5, 300), (11, 8, 400), (16, 13, 500), (20, 22, 1000)];

struct Uci {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    go: String,
}

impl Uci {
    fn start(path: &str, options: &[(&str, String)], go: String) -> Uci {
        let mut child = Command::new(path).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("start engine");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut uci = Uci { _child: child, stdin, stdout, go };
        uci.send("uci");
        uci.wait_for("uciok");
        for (name, value) in options {
            uci.send(&format!("setoption name {name} value {value}"));
        }
        uci.send("isready");
        uci.wait_for("readyok");
        uci
    }

    fn send(&mut self, s: &str) {
        writeln!(self.stdin, "{s}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn wait_for(&mut self, prefix: &str) -> String {
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

    fn best_move(&mut self, start_fen: &str, moves: &[String]) -> String {
        self.send(&format!("position fen {start_fen} moves {}", moves.join(" ")));
        let go = self.go.clone();
        self.send(&go);
        self.wait_for("bestmove").split_whitespace().nth(1).unwrap_or("0000").to_string()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let brain = Brain::load(&arg(&args, "--brain", "nets/kishi.nnue".to_string()));
    let engine: String = arg(&args, "--engine", "stockfish".into());
    let pairs: u32 = arg::<u32>(&args, "--games", 100) / 2;
    let threads: usize = arg(&args, "--threads", 8);
    // Threads for Kishi within each game (the phone gives it four); 1 keeps A/B tests comparable.
    let kishi_threads: usize = arg(&args, "--kishi-threads", 1);
    let ms: f64 = arg(&args, "--ms", 1000.0);
    let lichess: usize = arg(&args, "--lichess", 0);
    let elo_limit: u32 = arg(&args, "--elo", 0);
    let seed: u64 = arg(&args, "--seed", 11);
    // For A/B tests against an older Kishi: both sides search a fixed number of nodes.
    let nodes: u64 = arg(&args, "--nodes", 0);

    let (options, go, label): (Vec<(&str, String)>, String, String) = if nodes > 0 {
        (vec![("Hash", "16".into())], format!("go nodes {nodes}"), format!("{engine} at {nodes} nodes"))
    } else if lichess > 0 {
        let (skill, depth, movetime) = LICHESS[lichess - 1];
        (
            vec![("Threads", "1".into()), ("Hash", "16".into()), ("Skill Level", skill.max(0).to_string())],
            format!("go depth {depth} movetime {movetime}"),
            format!("lichess level {lichess}"),
        )
    } else {
        (
            vec![("Threads", "1".into()), ("Hash", "16".into()), ("UCI_LimitStrength", "true".into()), ("UCI_Elo", elo_limit.to_string())],
            format!("go movetime {}", ms as u64),
            format!("Stockfish UCI_Elo {elo_limit}"),
        )
    };

    let next = AtomicU32::new(0);
    let score = Mutex::new((0u32, 0u32, 0u32));
    std::thread::scope(|s| {
        for _ in 0..threads {
            let (brain, next, score, options, go, engine) = (brain.clone(), &next, &score, &options, &go, &engine);
            s.spawn(move || {
                let mut kishi = brain.searcher(64);
                let mut helpers: Vec<Searcher> = (1..kishi_threads).map(|_| kishi.helper()).collect();
                let mut judge = Brain::Handwritten.searcher(4);
                let mut other = Uci::start(engine, options, go.clone());
                loop {
                    let pair = next.fetch_add(1, Ordering::Relaxed);
                    if pair >= pairs {
                        break;
                    }
                    let mut rng = Rng((seed + pair as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
                    let (start, history) = random_opening(&mut rng, 8, &mut judge);
                    for kishi_white in [true, false] {
                        let limits = if nodes > 0 { Limits { nodes, ..Default::default() } } else { Limits::movetime(ms) };
                        let result = play(&mut kishi, &mut helpers, &mut other, start, history.clone(), kishi_white, limits);
                        let mut sc = score.lock().unwrap();
                        match result {
                            2 => sc.0 += 1,
                            1 => sc.1 += 1,
                            _ => sc.2 += 1,
                        }
                        let (w, d, l) = *sc;
                        if (w + d + l) % 10 == 0 {
                            let (e, err) = elo(w, d, l);
                            eprintln!("{} games  +{w} ={d} -{l}  elo {e:+.0} ± {err:.0}", w + d + l);
                        }
                    }
                }
                other.send("quit");
            });
        }
    });
    let (w, d, l) = *score.lock().unwrap();
    let (e, err) = elo(w, d, l);
    let budget = if nodes > 0 { format!("{nodes} nodes") } else { format!("{ms} ms") };
    let budget = if kishi_threads > 1 { format!("{budget}/move, {kishi_threads} threads") } else { format!("{budget}/move") };
    println!("Kishi ({budget}) vs {label}: +{w} ={d} -{l}  elo {e:+.1} ± {err:.1}");
}

/// One game; returns Kishi's result: 2 win, 1 draw, 0 loss.
fn play(kishi: &mut Searcher, helpers: &mut [Searcher], other: &mut Uci, start: Position, mut history: Vec<u64>, kishi_white: bool, limits: Limits) -> u8 {
    kishi.clear();
    other.send("ucinewgame");
    let start_fen = start.fen();
    let mut pos = start;
    let mut moves: Vec<String> = vec![];
    let mut streak = 0;
    loop {
        if pos.legal_moves().is_empty() {
            if !pos.in_check() {
                return 1;
            }
            let kishi_mated = (pos.stm == WHITE) == kishi_white;
            return if kishi_mated { 0 } else { 2 };
        }
        if pos.halfmove >= 100 || pos.insufficient_material() || history.iter().filter(|&&h| h == pos.hash).count() >= 2 || moves.len() > 400 {
            return 1;
        }
        let kishi_to_move = (pos.stm == WHITE) == kishi_white;
        let m = if kishi_to_move {
            let r = kishi.search_parallel(helpers, &pos, &history, limits, &mut |_| {});
            // Adjudicate a game Kishi is sure about, either way.
            if r.score.abs() >= 1500 || r.score.abs() >= MATE_BOUND {
                streak += 1;
                if streak >= 5 {
                    return if r.score > 0 { 2 } else { 0 };
                }
            } else {
                streak = 0;
            }
            r.best
        } else {
            let uci = other.best_move(&start_fen, &moves);
            match pos.parse_uci(&uci) {
                Some(m) => m,
                None => panic!("opponent played illegal {uci} in {}", pos.fen()),
            }
        };
        moves.push(m.uci());
        history.push(pos.hash);
        pos = pos.make(m);
    }
}
