//! Plays two evaluations against each other and reports the Elo difference.
//!
//!   arena --a nets/gen1.nnue --b hce [--games 400] [--nodes 20000 | --ms 100]
//!         [--threads 10] [--seed 7]
//!
//! Games come in pairs from the same random opening with colors swapped, so
//! neither side profits from a lucky opening.

use kishi_engine::Limits;
use kishi_engine::types::Rng;
use kishi_tools::*;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (a_spec, b_spec): (String, String) = (arg(&args, "--a", "hce".into()), arg(&args, "--b", "hce".into()));
    let (a, b) = (Brain::load(&a_spec), Brain::load(&b_spec));
    let pairs: u32 = arg::<u32>(&args, "--games", 400) / 2;
    let threads: usize = arg(&args, "--threads", std::thread::available_parallelism().map_or(4, |n| n.get()));
    let nodes: u64 = arg(&args, "--nodes", 0);
    let ms: f64 = arg(&args, "--ms", 0.0);
    let seed: u64 = arg(&args, "--seed", 7);
    let limits = if ms > 0.0 {
        Limits::movetime(ms)
    } else {
        let n = if nodes > 0 { nodes } else { 20_000 };
        Limits { soft_nodes: n, nodes: n * 8, ..Default::default() }
    };

    let next = AtomicU32::new(0);
    let score = Mutex::new((0u32, 0u32, 0u32));
    std::thread::scope(|s| {
        for _ in 0..threads {
            let (a, b, next, score) = (a.clone(), b.clone(), &next, &score);
            s.spawn(move || {
                let (mut sa, mut sb) = (a.searcher(16), b.searcher(16));
                let mut judge = Brain::Handwritten.searcher(4);
                loop {
                    let pair = next.fetch_add(1, Ordering::Relaxed);
                    if pair >= pairs {
                        break;
                    }
                    let mut rng = Rng((seed + pair as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
                    let (pos, history) = random_opening(&mut rng, 8, &mut judge);
                    for a_white in [true, false] {
                        let game = if a_white {
                            play_game(&mut sa, &mut sb, pos, history.clone(), limits)
                        } else {
                            play_game(&mut sb, &mut sa, pos, history.clone(), limits)
                        };
                        let a_score = match (game.outcome, a_white) {
                            (Outcome::Draw, _) => 1,
                            (Outcome::WhiteWins, true) | (Outcome::BlackWins, false) => 2,
                            _ => 0,
                        };
                        let mut s = score.lock().unwrap();
                        match a_score {
                            2 => s.0 += 1,
                            1 => s.1 += 1,
                            _ => s.2 += 1,
                        }
                        let (w, d, l) = *s;
                        let (e, err) = elo(w, d, l);
                        if (w + d + l) % 20 == 0 {
                            eprintln!("{} games  +{w} ={d} -{l}  elo {e:+.0} ± {err:.0}", w + d + l);
                        }
                    }
                }
            });
        }
    });
    let (w, d, l) = *score.lock().unwrap();
    let (e, err) = elo(w, d, l);
    println!("{a_spec} vs {b_spec}: +{w} ={d} -{l}  elo {e:+.1} ± {err:.1}");
}
