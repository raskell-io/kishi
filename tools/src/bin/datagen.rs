//! Self-play data generation.
//!
//!   datagen --brain hce|net.nnue --out data/gen0.bin --positions 20000000
//!           [--threads 10] [--nodes 5000] [--seed 1]
//!
//! Every thread plays games against itself from random openings and appends
//! the quiet positions, labelled with search score and game result.

use kishi_engine::Limits;
use kishi_engine::types::Rng;
use kishi_tools::*;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let brain = Brain::load(&arg(&args, "--brain", "hce".to_string()));
    let out_path: String = arg(&args, "--out", "data/selfplay.bin".to_string());
    let target: u64 = arg(&args, "--positions", 1_000_000);
    let threads: usize = arg(&args, "--threads", std::thread::available_parallelism().map_or(4, |n| n.get()));
    let nodes: u64 = arg(&args, "--nodes", 5000);
    let seed: u64 = arg(&args, "--seed", 1);

    if let Some(dir) = std::path::Path::new(&out_path).parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let mut out = std::io::BufWriter::new(std::fs::OpenOptions::new().create(true).append(true).open(&out_path).unwrap());
    let written = AtomicU64::new(0);
    let games = AtomicU64::new(0);
    let wdl = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
    let (tx, rx) = mpsc::sync_channel::<Vec<Record>>(256);
    let start = Instant::now();
    let limits = Limits { soft_nodes: nodes, nodes: nodes * 8, ..Default::default() };

    std::thread::scope(|s| {
        for t in 0..threads {
            let tx = tx.clone();
            let (brain, written, games, wdl) = (brain.clone(), &written, &games, &wdl);
            s.spawn(move || {
                let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (t as u64 + 1) * 0x2545_f491_4f6c_dd1d);
                let mut a = brain.searcher(16);
                let mut b = brain.searcher(16);
                while written.load(Ordering::Relaxed) < target {
                    let plies = 8 + rng.below(2);
                    let (pos, history) = random_opening(&mut rng, plies, &mut a);
                    let game = play_game(&mut a, &mut b, pos, history, limits);
                    wdl[game.outcome.code() as usize].fetch_add(1, Ordering::Relaxed);
                    games.fetch_add(1, Ordering::Relaxed);
                    written.fetch_add(game.positions.len() as u64, Ordering::Relaxed);
                    if tx.send(game.positions).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        let mut last_report = Instant::now();
        let mut total = 0u64;
        for batch in rx {
            for r in &batch {
                out.write_all(&r.to_bytes()).unwrap();
            }
            total += batch.len() as u64;
            if last_report.elapsed().as_secs() >= 10 {
                last_report = Instant::now();
                let secs = start.elapsed().as_secs_f64();
                eprintln!(
                    "{total} positions, {} games (+{} ={} -{}), {:.0} pos/s",
                    games.load(Ordering::Relaxed),
                    wdl[2].load(Ordering::Relaxed),
                    wdl[1].load(Ordering::Relaxed),
                    wdl[0].load(Ordering::Relaxed),
                    total as f64 / secs
                );
            }
        }
        out.flush().unwrap();
        eprintln!("done: {total} positions in {:.0}s -> {out_path}", start.elapsed().as_secs_f64());
    });
}
