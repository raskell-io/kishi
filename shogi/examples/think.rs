//! Searches a position for a while and prints what it found, for a quick look at speed and depth.
//!   cargo run --release -p kishi-shogi --example think -- [ms] [sfen...]
use kishi_shogi::*;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ms: f64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(2000.0);
    let pos = if args.len() > 1 { Position::from_sfen(&args[1..].join(" ")).unwrap() } else { Position::startpos() };
    let mut s = Searcher::new(64);
    let t = std::time::Instant::now();
    let r = s.search(&pos, &[], Limits { soft_ms: ms / 2.0, hard_ms: ms, ..Default::default() });
    let secs = t.elapsed().as_secs_f64();
    println!("{} ({}) score {} depth {} nodes {} nps {:.0}", r.best.usi(), pos.notation(r.best), r.score, r.depth, r.nodes, r.nodes as f64 / secs);
}
