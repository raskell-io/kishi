//! Kishi over UCI, for chess GUIs and match runners.
//!
//! Extra commands: `bench` (fixed-depth search over a position set, prints
//! nodes per second), `perft <depth>`, `eval`.

use kishi_engine::nnue::Network;
use kishi_engine::search::Info;
use kishi_engine::*;
use std::io::{BufRead, Write};

pub const BENCH_FENS: &[&str] = &[
    START_FEN,
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    "rnbqkb1r/pp1p1ppp/4pn2/2p5/2PP4/2N5/PP2PPPP/R1BQKBNR w KQkq - 0 4",
    "r1bq1rk1/pp2ppbp/2np1np1/8/3NP3/2N1BP2/PPPQ2PP/R3KB1R w KQ - 3 9",
    "2r3k1/pp3ppp/4p3/3pP3/1q1P4/3Q1N2/P4PPP/2R3K1 w - - 0 24",
    "8/8/4k3/3p4/3P4/4K3/8/8 w - - 0 1",
    "6k1/5p2/6p1/8/7p/8/6PP/6K1 b - - 0 1",
    "r1bqk2r/pppp1ppp/2n2n2/2b1p3/2B1P3/5N2/PPPP1PPP/RNBQ1RK1 w kq - 6 5",
    "3r1rk1/p4ppp/1qp1b3/2b1p3/4P3/1BN1QP2/PPP3PP/2KR3R w - - 0 17",
    "8/5pk1/7p/3R4/6P1/5K1P/r7/8 b - - 0 45",
];

fn print_info(info: &Info) {
    let score = if info.score.abs() >= search::MATE_BOUND {
        let plies = search::MATE - info.score.abs();
        format!("mate {}", if info.score > 0 { (plies + 1) / 2 } else { -(plies + 1) / 2 })
    } else {
        format!("cp {}", info.score)
    };
    let pv: Vec<String> = info.pv.iter().map(|m| m.uci()).collect();
    let nps = (info.nodes as f64 / (info.time_ms / 1000.0).max(1e-3)) as u64;
    println!(
        "info depth {} seldepth {} score {score} nodes {} nps {nps} time {} hashfull {} pv {}",
        info.depth,
        info.seldepth,
        info.nodes,
        info.time_ms as u64,
        info.hashfull,
        pv.join(" ")
    );
    std::io::stdout().flush().ok();
}

fn main() {
    let mut hash_mb = 64;
    let mut searcher = Searcher::new(hash_mb);
    let mut helpers: Vec<Searcher> = vec![];
    let mut pos = Position::startpos();
    let mut history: Vec<u64> = vec![];

    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--net") {
        let bytes = std::fs::read(&args[i + 1]).expect("read network");
        searcher.set_network(Some(Box::leak(Box::new(Network::from_bytes(&bytes).expect("parse network")))));
    }
    if args.iter().any(|a| a == "--hce") {
        searcher.set_network(None);
    }
    if args.iter().any(|a| a == "bench") {
        bench(&mut searcher, 12);
        return;
    }

    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.first().copied() {
            Some("uci") => {
                println!("id name Kishi {}", env!("CARGO_PKG_VERSION"));
                println!("id author raskell.io");
                println!("option name Hash type spin default 64 min 1 max 4096");
                println!("option name Threads type spin default 1 min 1 max 64");
                println!("uciok");
            }
            Some("isready") => println!("readyok"),
            Some("setoption") => {
                if let (Some(n), Some(v)) = (words.iter().position(|w| *w == "name"), words.iter().position(|w| *w == "value")) {
                    if words[n + 1].eq_ignore_ascii_case("hash") {
                        hash_mb = words[v + 1].parse().unwrap_or(hash_mb);
                        searcher.tt = std::sync::Arc::new(tt::Tt::new(hash_mb));
                        let n = helpers.len();
                        helpers = (0..n).map(|_| searcher.helper()).collect();
                    }
                    if words[n + 1].eq_ignore_ascii_case("threads") {
                        let n: usize = words[v + 1].parse().unwrap_or(1);
                        helpers = (1..n.max(1)).map(|_| searcher.helper()).collect();
                    }
                }
            }
            Some("ucinewgame") => {
                searcher.clear();
                for h in &mut helpers {
                    h.clear();
                }
            }
            Some("position") => {
                let (p, h) = parse_position(&words[1..]);
                pos = p;
                history = h;
            }
            Some("go") => {
                let limits = parse_go(&words[1..], pos.stm);
                let r = searcher.search_parallel(&mut helpers, &pos, &history, limits, &mut print_info);
                println!("bestmove {}", r.best.uci());
            }
            Some("perft") => {
                let d = words.get(1).and_then(|s| s.parse().ok()).unwrap_or(5);
                let t = std::time::Instant::now();
                let n = movegen::perft(&pos, d);
                println!("nodes {n} time {:?}", t.elapsed());
            }
            Some("eval") => println!("eval {}", searcher.evaluate(&pos)),
            Some("bench") => bench(&mut searcher, words.get(1).and_then(|s| s.parse().ok()).unwrap_or(12)),
            Some("d") => println!("{}", pos.fen()),
            Some("quit") => break,
            _ => {}
        }
        std::io::stdout().flush().ok();
    }
}

fn parse_position(words: &[&str]) -> (Position, Vec<u64>) {
    let moves_at = words.iter().position(|w| *w == "moves").unwrap_or(words.len());
    let mut pos = match words.first() {
        Some(&"fen") => Position::from_fen(&words[1..moves_at].join(" ")).unwrap_or_else(|_| Position::startpos()),
        _ => Position::startpos(),
    };
    let mut history = vec![];
    for w in words.iter().skip(moves_at + 1) {
        match pos.parse_uci(w) {
            Some(m) => {
                history.push(pos.hash);
                pos = pos.make(m);
            }
            None => break,
        }
    }
    (pos, history)
}

fn parse_go(words: &[&str], stm: usize) -> Limits {
    let get = |key: &str| words.iter().position(|w| *w == key).and_then(|i| words.get(i + 1)).and_then(|v| v.parse::<f64>().ok());
    if let Some(d) = get("depth") {
        return Limits::depth(d as i32);
    }
    if let Some(n) = get("nodes") {
        return Limits { nodes: n as u64, ..Default::default() };
    }
    if let Some(t) = get("movetime") {
        return Limits::movetime(t);
    }
    let (time, inc) = if stm == 0 { (get("wtime"), get("winc")) } else { (get("btime"), get("binc")) };
    match time {
        Some(t) => Limits::clock(t, inc.unwrap_or(0.0), get("movestogo").map(|m| m as u32)),
        None => Limits::default(),
    }
}

fn bench(searcher: &mut Searcher, depth: i32) {
    let (mut nodes, mut time) = (0u64, 0.0);
    for fen in BENCH_FENS {
        searcher.clear();
        let pos = Position::from_fen(fen).unwrap();
        let t = search::now_ms();
        let r = searcher.search(&pos, &[], Limits::depth(depth), &mut |_| {});
        time += search::now_ms() - t;
        nodes += r.nodes;
        println!("{:>10} {:>6} {:>7}  {fen}", r.nodes, r.best.uci(), r.score);
    }
    println!("{nodes} nodes {} nps", (nodes as f64 / (time / 1000.0)) as u64);
}
