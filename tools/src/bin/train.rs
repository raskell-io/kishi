//! Trains the evaluation network on self-play records.
//!
//!   train --data data/gen0.bin[,data/gen1.bin...] --out nets/gen1.nnue
//!         [--epochs 30] [--lr 0.001] [--wdl 0.3] [--batch 16384] [--threads 10]
//!
//! The network predicts the game's outcome. Its target blends the search
//! score of the position (as a win probability) with the result of the game
//! it came from; the result is the reinforcement signal that lets each
//! generation correct the evaluation that played it.

use kishi_engine::nnue::{BUCKETS, HIDDEN, INPUTS, Network, QA, SCALE, feature};
use kishi_engine::types::{Rng, WHITE};
use kishi_tools::*;
use std::time::Instant;

const OUT: usize = 2 * HIDDEN;
const K: f32 = 400.0;

/// Every parameter, flat: w1, b1, w2, b2.
struct Params(Vec<f32>);

const W1: usize = 0;
const B1: usize = W1 + INPUTS * HIDDEN;
const W2: usize = B1 + HIDDEN;
const B2: usize = W2 + BUCKETS * OUT;
const N_PARAMS: usize = B2 + BUCKETS;

struct Sample {
    stm: [u16; 32],
    nstm: [u16; 32],
    n: usize,
    bucket: usize,
    target: f32,
}

fn sample(r: &Record, wdl: f32) -> Sample {
    let mut s = Sample { stm: [0; 32], nstm: [0; 32], n: 0, bucket: 0, target: 0.0 };
    let stm = r.stm as usize;
    for (sq, p) in r.pieces() {
        s.stm[s.n] = feature(stm, p, sq) as u16;
        s.nstm[s.n] = feature(stm ^ 1, p, sq) as u16;
        s.n += 1;
    }
    s.bucket = (s.n.saturating_sub(2) / 4).min(BUCKETS - 1);
    let (score, result) = if stm == WHITE {
        (r.score as f32, r.result as f32 / 2.0)
    } else {
        (-r.score as f32, 1.0 - r.result as f32 / 2.0)
    };
    s.target = wdl * result + (1.0 - wdl) * sigmoid(score / K);
    s
}

#[inline(always)]
fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// Forward and backward pass over `samples`, accumulating into `grad`. Returns the summed loss.
fn run(params: &[f32], samples: &[Sample], grad: Option<&mut [f32]>) -> f64 {
    let mut acc = [[0f32; HIDDEN]; 2];
    let mut act = [[0f32; HIDDEN]; 2];
    let mut loss = 0f64;
    let mut grad = grad;
    for s in samples {
        for (side, feats) in [&s.stm, &s.nstm].into_iter().enumerate() {
            let a = &mut acc[side];
            a.copy_from_slice(&params[B1..B1 + HIDDEN]);
            for &f in &feats[..s.n] {
                let row = &params[W1 + f as usize * HIDDEN..W1 + (f as usize + 1) * HIDDEN];
                for (x, w) in a.iter_mut().zip(row) {
                    *x += w;
                }
            }
            for (y, x) in act[side].iter_mut().zip(a.iter()) {
                let c = x.clamp(0.0, 1.0);
                *y = c * c;
            }
        }
        let w2 = &params[W2 + s.bucket * OUT..W2 + (s.bucket + 1) * OUT];
        let mut out = params[B2 + s.bucket];
        for i in 0..HIDDEN {
            out += act[0][i] * w2[i] + act[1][i] * w2[HIDDEN + i];
        }
        let pred = sigmoid(out * SCALE as f32 / K);
        let err = pred - s.target;
        loss += (err * err) as f64;

        let Some(g) = grad.as_deref_mut() else { continue };
        let d_out = 2.0 * err * pred * (1.0 - pred) * SCALE as f32 / K;
        g[B2 + s.bucket] += d_out;
        let (g_head, g_w2) = g.split_at_mut(W2);
        let gw2 = &mut g_w2[s.bucket * OUT..(s.bucket + 1) * OUT];
        for side in 0..2 {
            let mut d_acc = [0f32; HIDDEN];
            for i in 0..HIDDEN {
                let j = side * HIDDEN + i;
                gw2[j] += d_out * act[side][i];
                let x = acc[side][i];
                d_acc[i] = if x > 0.0 && x < 1.0 { d_out * w2[j] * 2.0 * x } else { 0.0 };
            }
            let feats = if side == 0 { &s.stm } else { &s.nstm };
            for &f in &feats[..s.n] {
                let row = &mut g_head[W1 + f as usize * HIDDEN..W1 + (f as usize + 1) * HIDDEN];
                for (r, d) in row.iter_mut().zip(&d_acc) {
                    *r += d;
                }
            }
            for (b, d) in g_head[B1..B1 + HIDDEN].iter_mut().zip(&d_acc) {
                *b += d;
            }
        }
    }
    loss
}

fn quantise(p: &[f32]) -> Network {
    let q = |x: f32, scale: i32, limit: f32| (x.clamp(-limit, limit) * scale as f32).round();
    Network {
        w1: p[W1..B1].iter().map(|&x| q(x, QA, 127.0) as i16).collect(),
        b1: p[B1..W2].iter().map(|&x| q(x, QA, 127.0) as i16).collect(),
        w2: p[W2..B2].to_vec(),
        b2: p[B2..].to_vec(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data: String = arg(&args, "--data", "data/gen0.bin".to_string());
    let out: String = arg(&args, "--out", "nets/trained.nnue".to_string());
    let epochs: usize = arg(&args, "--epochs", 30);
    let lr0: f32 = arg(&args, "--lr", 0.001);
    let wdl: f32 = arg(&args, "--wdl", 0.3);
    let batch: usize = arg(&args, "--batch", 16384);
    let threads: usize = arg(&args, "--threads", std::thread::available_parallelism().map_or(4, |n| n.get()));
    let seed: u64 = arg(&args, "--seed", 1);

    // Load and shuffle every record once; each epoch then visits batches in a fresh order.
    let mut records: Vec<Record> = vec![];
    for path in data.split(',') {
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
        records.extend(bytes.chunks_exact(RECORD_SIZE).map(Record::from_bytes));
        eprintln!("{path}: {} records", bytes.len() / RECORD_SIZE);
    }
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    for i in (1..records.len()).rev() {
        records.swap(i, rng.below(i + 1));
    }
    let n_valid = (records.len() / 100).min(200_000);
    let valid: Vec<Sample> = records[..n_valid].iter().map(|r| sample(r, wdl)).collect();
    let train = &records[n_valid..];
    let n_batches = train.len() / batch;
    eprintln!("{} training, {n_valid} validation, {n_batches} batches of {batch}, {threads} threads", train.len());

    let mut params = Params(vec![0f32; N_PARAMS]);
    for w in &mut params.0[W1..B1] {
        *w = (rng.unit() as f32 * 2.0 - 1.0) * 0.08;
    }
    for w in &mut params.0[W2..B2] {
        *w = (rng.unit() as f32 * 2.0 - 1.0) * (1.0 / (OUT as f32).sqrt());
    }
    let (mut m, mut v) = (vec![0f32; N_PARAMS], vec![0f32; N_PARAMS]);
    let (beta1, beta2, eps) = (0.9f32, 0.999f32, 1e-8f32);
    let mut grads: Vec<Vec<f32>> = (0..threads).map(|_| vec![0f32; N_PARAMS]).collect();
    let mut step = 0i32;
    let start = Instant::now();

    let validate = |p: &[f32]| -> f64 {
        let chunk = valid.len().div_ceil(threads);
        std::thread::scope(|s| {
            let handles: Vec<_> = valid.chunks(chunk).map(|c| s.spawn(move || run(p, c, None))).collect();
            handles.into_iter().map(|h| h.join().unwrap()).sum::<f64>()
        }) / valid.len() as f64
    };

    for epoch in 1..=epochs {
        // Cosine decay from lr0 to lr0 / 50.
        let progress = (epoch - 1) as f32 / epochs.max(2) as f32;
        let lr = lr0 / 50.0 + (lr0 - lr0 / 50.0) * 0.5 * (1.0 + (std::f32::consts::PI * progress).cos());
        let mut order: Vec<usize> = (0..n_batches).collect();
        for i in (1..order.len()).rev() {
            order.swap(i, rng.below(i + 1));
        }
        let mut train_loss = 0f64;
        for &b in &order {
            let slice = &train[b * batch..(b + 1) * batch];
            let per = batch.div_ceil(threads);
            let p = &params.0;
            train_loss += std::thread::scope(|s| {
                let handles: Vec<_> = slice
                    .chunks(per)
                    .zip(grads.iter_mut())
                    .map(|(c, g)| {
                        s.spawn(move || {
                            g.fill(0.0);
                            let samples: Vec<Sample> = c.iter().map(|r| sample(r, wdl)).collect();
                            run(p, &samples, Some(g))
                        })
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap()).sum::<f64>()
            });
            // Reduce the per-thread gradients and take an Adam step, in parallel over parameters.
            step += 1;
            let (c1, c2) = (1.0 - beta1.powi(step), 1.0 - beta2.powi(step));
            let scale = 1.0 / batch as f32;
            let span = N_PARAMS.div_ceil(threads);
            let grads_ref = &grads;
            std::thread::scope(|s| {
                for (t, ((pc, mc), vc)) in params.0.chunks_mut(span).zip(m.chunks_mut(span)).zip(v.chunks_mut(span)).enumerate() {
                    s.spawn(move || {
                        let base = t * span;
                        for i in 0..pc.len() {
                            let mut g = 0f32;
                            for gr in grads_ref {
                                g += gr[base + i];
                            }
                            g *= scale;
                            mc[i] = beta1 * mc[i] + (1.0 - beta1) * g;
                            vc[i] = beta2 * vc[i] + (1.0 - beta2) * g * g;
                            pc[i] -= lr * (mc[i] / c1) / ((vc[i] / c2).sqrt() + eps);
                        }
                    });
                }
            });
        }
        let vl = validate(&params.0);
        eprintln!(
            "epoch {epoch:>3}  lr {lr:.6}  train {:.6}  valid {vl:.6}  {:.0}s",
            train_loss / (n_batches * batch) as f64,
            start.elapsed().as_secs_f64()
        );
        std::fs::write(&out, quantise(&params.0).to_bytes()).unwrap();
    }
    eprintln!("wrote {out}");

    // The engine's integer arithmetic should agree with the float network.
    let net = quantise(&params.0);
    let mut searcher = kishi_engine::Searcher::new(1);
    searcher.set_network(Some(Box::leak(Box::new(net))));
    let (mut worst, mut total) = (0f32, 0f32);
    for r in records.iter().take(2000) {
        let s = sample(r, wdl);
        let float = forward(&params.0, &s) * SCALE as f32;
        let int = searcher.evaluate(&r.position()) as f32;
        worst = worst.max((float - int).abs());
        total += (float - int).abs();
    }
    eprintln!("float vs quantised over 2000 positions: mean {:.1} cp, largest {worst:.1} cp", total / 2000.0);
}

/// The network's output for one sample, in units of SCALE centipawns.
fn forward(p: &[f32], s: &Sample) -> f32 {
    let mut out = p[B2 + s.bucket];
    for (side, feats) in [&s.stm, &s.nstm].into_iter().enumerate() {
        let mut acc = p[B1..B1 + HIDDEN].to_vec();
        for &f in &feats[..s.n] {
            for (x, w) in acc.iter_mut().zip(&p[W1 + f as usize * HIDDEN..]) {
                *x += w;
            }
        }
        for (i, x) in acc.iter().enumerate() {
            let c = x.clamp(0.0, 1.0);
            out += c * c * p[W2 + s.bucket * OUT + side * HIDDEN + i];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kishi_engine::Position;

    /// Backpropagation agrees with finite differences of the loss.
    #[test]
    fn gradients_match_finite_differences() {
        let mut rng = Rng(3);
        let mut p = vec![0f32; N_PARAMS];
        for w in &mut p {
            *w = (rng.unit() as f32 * 2.0 - 1.0) * 0.15;
        }
        let fens = [
            "r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4",
            "8/5pk1/7p/3R4/6P1/5K1P/r7/8 b - - 0 45",
            "2r3k1/pp3ppp/4p3/3pP3/1q1P4/3Q1N2/P4PPP/2R3K1 w - - 0 24",
        ];
        let samples: Vec<Sample> = fens
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let mut r = Record::new(&Position::from_fen(f).unwrap(), (i as i16 - 1) * 150);
                r.result = i as u8;
                sample(&r, 0.4)
            })
            .collect();
        let mut grad = vec![0f32; N_PARAMS];
        run(&p, &samples, Some(&mut grad));
        // The parameters these positions touch: their first-layer rows, every bias and output weight.
        let mut checked = 0;
        for s in &samples {
            for &f in &s.stm[..3] {
                let idx = W1 + f as usize * HIDDEN + rng.below(HIDDEN);
                check(&mut p, &samples, &grad, idx);
                checked += 1;
            }
        }
        for _ in 0..20 {
            check(&mut p, &samples, &grad, B1 + rng.below(HIDDEN));
            check(&mut p, &samples, &grad, W2 + samples[0].bucket * OUT + rng.below(OUT));
            checked += 2;
        }
        check(&mut p, &samples, &grad, B2 + samples[1].bucket);
        assert!(checked > 40);
    }

    fn check(p: &mut [f32], samples: &[Sample], grad: &[f32], idx: usize) {
        let h = 1e-3;
        let old = p[idx];
        p[idx] = old + h;
        let up = run(p, samples, None);
        p[idx] = old - h;
        let down = run(p, samples, None);
        p[idx] = old;
        let numeric = ((up - down) / (2.0 * h as f64)) as f32;
        let tolerance = 1e-3 + 0.05 * numeric.abs().max(grad[idx].abs());
        assert!((numeric - grad[idx]).abs() <= tolerance, "param {idx}: analytic {} numeric {numeric}", grad[idx]);
    }
}
