//! Plays Kishi's Go engine against GNU Go on 19x19, Chinese rules, komi 7.5.
//!
//!   go-vs [--level 1] [--playouts 20000 | --ms 2000] [--games 10] [--threads 5]
//!
//! Colours alternate. A game ends after two passes, a resignation, or 600 moves,
//! and GNU Go's final_score decides it.

use kishi_go::board::{BLACK, PASS, name, parse};
use kishi_go::*;
use kishi_go::mcts;
use kishi_tools::{arg, elo};
use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

struct Gtp {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Gtp {
    fn start(level: u32) -> Gtp {
        let mut child = Command::new("gnugo")
            .args(["--mode", "gtp", "--level", &level.to_string(), "--chinese-rules", "--komi", "7.5"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start gnugo");
        let g = Gtp { stdin: child.stdin.take().unwrap(), stdout: BufReader::new(child.stdout.take().unwrap()) };
        std::mem::forget(child);
        g
    }
    fn cmd(&mut self, c: &str) -> String {
        writeln!(self.stdin, "{c}").unwrap();
        self.stdin.flush().unwrap();
        let mut out = String::new();
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).unwrap();
            if line.trim().is_empty() && !out.is_empty() {
                return out.trim_start_matches(['=', '?', ' ']).trim().to_string();
            }
            out.push_str(&line);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let level: u32 = arg(&args, "--level", 1);
    let playouts: u32 = arg(&args, "--playouts", u32::MAX);
    let ms: f64 = arg(&args, "--ms", if playouts == u32::MAX { 2000.0 } else { f64::INFINITY });
    let games: u32 = arg(&args, "--games", 10);
    let threads: usize = arg(&args, "--threads", 5);
    let resign = !args.iter().any(|a| a == "--no-resign");
    let next = AtomicU32::new(0);
    let score = Mutex::new((0u32, 0u32));
    std::thread::scope(|s| {
        for _ in 0..threads {
            let (next, score) = (&next, &score);
            s.spawn(move || {
                let mut gnu = Gtp::start(level);
                loop {
                    let g = next.fetch_add(1, Ordering::Relaxed);
                    if g >= games {
                        break;
                    }
                    let kishi_black = g % 2 == 0;
                    let mut kishi = Searcher::new(g as u64 * 7919 + 1);
                    let (win, how) = play(&mut kishi, &mut gnu, kishi_black, Limits { playouts, ms }, resign);
                    let mut sc = score.lock().unwrap();
                    if win { sc.0 += 1 } else { sc.1 += 1 }
                    eprintln!("game {g}: Kishi {} as {} ({how})  total +{} -{}", if win { "wins" } else { "loses" }, if kishi_black { "black" } else { "white" }, sc.0, sc.1);
                }
                gnu.cmd("quit");
            });
        }
    });
    let (w, l) = *score.lock().unwrap();
    let (e, err) = elo(w, 0, l);
    println!("Kishi go vs GNU Go level {level}: +{w} -{l}  elo {e:+.0} ± {err:.0}");
}

fn play(kishi: &mut Searcher, gnu: &mut Gtp, kishi_black: bool, limits: Limits, resign: bool) -> (bool, String) {
    gnu.cmd("boardsize 19");
    gnu.cmd("clear_board");
    let mut board = Board::new();
    let mut history = vec![board.hash];
    while board.passes < 2 && board.moves < 600 {
        let kishi_to_move = (board.to_move == BLACK) == kishi_black;
        let colour = if board.to_move == BLACK { "b" } else { "w" };
        let m = if kishi_to_move {
            let (d, r) = kishi.decide(&board, &history, limits);
            if std::env::var("GO_TRACE").is_ok() {
                eprintln!("    kishi winrate {:.2} after {} playouts", r.winrate, r.playouts);
            }
            let m = match d {
                mcts::Decision::Resign if resign => return (false, "resigned".into()),
                mcts::Decision::Pass | mcts::Decision::Resign => PASS,
                mcts::Decision::Play(m) => m,
            };
            gnu.cmd(&format!("play {colour} {}", name(m)));
            m
        } else {
            let reply = gnu.cmd(&format!("genmove {colour}"));
            if reply.eq_ignore_ascii_case("resign") {
                return (true, "GNU Go resigned".into());
            }
            let m = parse(&reply).unwrap_or_else(|| panic!("unreadable move {reply}"));
            assert!(m == PASS || board.is_legal(m, board.to_move), "GNU Go's {reply} is illegal for Kishi");
            m
        };
        if std::env::var("GO_TRACE").is_ok() {
            eprintln!("{:>3} {} {}{}", board.moves + 1, colour, name(m), if kishi_to_move { " (kishi)" } else { "" });
        }
        board.play(m);
        history.push(board.hash);
    }
    if std::env::var("GO_TRACE").is_ok() {
        eprintln!("{}", gnu.cmd("showboard"));
    }
    let result = gnu.cmd("final_score");
    let black_won = result.starts_with('B');
    (black_won == kishi_black, result)
}
