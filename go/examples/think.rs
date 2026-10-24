//! Searches the empty board for a while and prints the speed: `cargo run --release -p kishi-go --example think -- [ms]`.
use kishi_go::board::name;
use kishi_go::*;

fn main() {
    let ms: f64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(3000.0);
    let mut s = Searcher::new(1);
    let t = std::time::Instant::now();
    let r = s.search(&Board::new(), &[], Limits { playouts: u32::MAX, ms });
    let secs = t.elapsed().as_secs_f64();
    println!("{} winrate {:.2} playouts {} ({:.0}/s)", name(r.best), r.winrate, r.playouts, r.playouts as f64 / secs);
}
