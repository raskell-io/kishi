//! NNUE inference: (768 -> HIDDEN) x 2 perspectives -> 1, with squared
//! clipped ReLU and output buckets chosen by piece count.
//!
//! The first layer is kept as an accumulator per perspective and updated
//! incrementally as moves are made; its weights are quantised to i16 by QA.
//! The output layer stays in f32: rounding its 1024 weights to i16 cost more
//! than 10 centipawns on average.

use crate::position::Position;
use crate::types::*;

pub const INPUTS: usize = 768;
pub const HIDDEN: usize = 512;
pub const BUCKETS: usize = 8;
pub const QA: i32 = 255;
pub const SCALE: i32 = 400;
const MAGIC: &[u8; 4] = b"KSHI";

pub struct Network {
    /// Feature-major: the HIDDEN weights of feature f start at f * HIDDEN.
    pub w1: Vec<i16>,
    pub b1: Vec<i16>,
    /// Per bucket: HIDDEN weights for the side to move, then HIDDEN for the other side.
    pub w2: Vec<f32>,
    pub b2: Vec<f32>,
}

/// The input index of a piece on a square, seen from one side.
#[inline(always)]
pub fn feature(perspective: usize, p: u8, sq: usize) -> usize {
    let (color, kind) = (color_of(p), kind_of(p));
    if perspective == WHITE {
        (color != WHITE) as usize * 384 + kind * 64 + sq
    } else {
        (color != BLACK) as usize * 384 + kind * 64 + (sq ^ 56)
    }
}

#[inline(always)]
pub fn bucket(pos: &Position) -> usize {
    ((pos.occupied().count_ones() as usize).saturating_sub(2) / 4).min(BUCKETS - 1)
}

impl Network {
    /// Parses the file the trainer writes: "KSHI", hidden size, bucket count, then
    /// little-endian w1, b1 (i16), w2 and b2 (f32).
    pub fn from_bytes(bytes: &[u8]) -> Result<Network, String> {
        if bytes.len() < 12 || &bytes[..4] != MAGIC {
            return Err("not a Kishi network".into());
        }
        let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
        let (hidden, buckets) = (u32_at(4), u32_at(8));
        if hidden != HIDDEN || buckets != BUCKETS {
            return Err(format!("network is {hidden}x{buckets}, engine expects {HIDDEN}x{BUCKETS}"));
        }
        let expected = 12 + 2 * (INPUTS * HIDDEN + HIDDEN) + 4 * (BUCKETS * 2 * HIDDEN + BUCKETS);
        if bytes.len() != expected {
            return Err(format!("network has {} bytes, expected {expected}", bytes.len()));
        }
        let mut at = 12;
        let mut i16s = |n: usize| {
            let v: Vec<i16> = bytes[at..at + 2 * n].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
            at += 2 * n;
            v
        };
        let w1 = i16s(INPUTS * HIDDEN);
        let b1 = i16s(HIDDEN);
        let f32s: Vec<f32> = bytes[at..].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        let (w2, b2) = f32s.split_at(BUCKETS * 2 * HIDDEN);
        Ok(Network { w1, b1, w2: w2.to_vec(), b2: b2.to_vec() })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend((HIDDEN as u32).to_le_bytes());
        out.extend((BUCKETS as u32).to_le_bytes());
        for v in [&self.w1, &self.b1] {
            out.extend(v.iter().flat_map(|x| x.to_le_bytes()));
        }
        for v in [&self.w2, &self.b2] {
            out.extend(v.iter().flat_map(|x| x.to_le_bytes()));
        }
        out
    }

    /// The network compiled into the engine, if one was present at build time.
    pub fn embedded() -> Option<&'static Network> {
        static NET: std::sync::OnceLock<Option<Network>> = std::sync::OnceLock::new();
        NET.get_or_init(|| {
            let bytes = include_bytes!("../../nets/kishi.nnue");
            if bytes.is_empty() { None } else { Network::from_bytes(bytes).ok() }
        })
        .as_ref()
    }

    pub fn refresh(&self, acc: &mut Accumulator, pos: &Position) {
        for perspective in [WHITE, BLACK] {
            let a = &mut acc.v[perspective];
            a.copy_from_slice(&self.b1);
            for sq in Squares(pos.occupied()) {
                add(a, self.row(feature(perspective, pos.board[sq], sq)));
            }
        }
    }

    /// Writes into `dst` the accumulator for `after`, given `src` for `before`:
    /// one pass per perspective over the rows of the squares that changed.
    pub fn update(&self, dst: &mut Accumulator, src: &Accumulator, before: &Position, after: &Position) {
        let (mut adds, mut subs) = ([(0u8, 0usize); 4], [(0u8, 0usize); 4]);
        let (mut na, mut ns) = (0, 0);
        let changed = before.occupied() | after.occupied();
        for sq in Squares(changed) {
            let (old, new) = (before.board[sq], after.board[sq]);
            if old == new {
                continue;
            }
            if old != NO_PIECE {
                subs[ns] = (old, sq);
                ns += 1;
            }
            if new != NO_PIECE {
                adds[na] = (new, sq);
                na += 1;
            }
        }
        for perspective in [WHITE, BLACK] {
            let (mut a, mut s): ([&[i16]; 4], [&[i16]; 4]) = ([&[]; 4], [&[]; 4]);
            for i in 0..na {
                a[i] = self.row(feature(perspective, adds[i].0, adds[i].1));
            }
            for i in 0..ns {
                s[i] = self.row(feature(perspective, subs[i].0, subs[i].1));
            }
            let (out, inp) = (&mut dst.v[perspective], &src.v[perspective]);
            // Fixed-length views let the loops below compile without bounds checks.
            match (na, ns) {
                (1, 1) => {
                    let (a0, s0) = (fixed(a[0]), fixed(s[0]));
                    for i in 0..HIDDEN {
                        out[i] = inp[i].wrapping_add(a0[i]).wrapping_sub(s0[i]);
                    }
                }
                (1, 2) => {
                    let (a0, s0, s1) = (fixed(a[0]), fixed(s[0]), fixed(s[1]));
                    for i in 0..HIDDEN {
                        out[i] = inp[i].wrapping_add(a0[i]).wrapping_sub(s0[i]).wrapping_sub(s1[i]);
                    }
                }
                (2, 2) => {
                    let (a0, a1, s0, s1) = (fixed(a[0]), fixed(a[1]), fixed(s[0]), fixed(s[1]));
                    for i in 0..HIDDEN {
                        out[i] = inp[i].wrapping_add(a0[i]).wrapping_add(a1[i]).wrapping_sub(s0[i]).wrapping_sub(s1[i]);
                    }
                }
                _ => {
                    out.copy_from_slice(inp);
                    for r in &a[..na] {
                        add(out, r);
                    }
                    for r in &s[..ns] {
                        sub(out, r);
                    }
                }
            }
        }
    }

    #[inline(always)]
    fn row(&self, f: usize) -> &[i16] {
        &self.w1[f * HIDDEN..(f + 1) * HIDDEN]
    }

    /// Centipawns from the side to move's point of view.
    pub fn evaluate(&self, acc: &Accumulator, pos: &Position) -> i32 {
        let b = bucket(pos);
        let w = &self.w2[b * 2 * HIDDEN..(b + 1) * 2 * HIDDEN];
        let sum = screlu_dot(&acc.v[pos.stm], &w[..HIDDEN]) + screlu_dot(&acc.v[pos.stm ^ 1], &w[HIDDEN..]);
        ((sum / (QA * QA) as f32 + self.b2[b]) * SCALE as f32) as i32
    }
}

/// A first-layer row as a fixed-length array, so loops over it need no bounds checks.
#[inline(always)]
fn fixed(r: &[i16]) -> &[i16; HIDDEN] {
    r.try_into().unwrap()
}

#[inline(always)]
fn add(a: &mut [i16; HIDDEN], w: &[i16]) {
    for (x, y) in a.iter_mut().zip(w) {
        *x = x.wrapping_add(*y);
    }
}

#[inline(always)]
fn sub(a: &mut [i16; HIDDEN], w: &[i16]) {
    for (x, y) in a.iter_mut().zip(w) {
        *x = x.wrapping_sub(*y);
    }
}

/// Sum of clamp(x, 0, QA)^2 * w. The square is exact in i32; sixteen
/// independent lanes keep the float adds vectorised and pipelined.
#[inline(always)]
fn screlu_dot(x: &[i16; HIDDEN], w: &[f32]) -> f32 {
    let w: &[f32; HIDDEN] = w.try_into().unwrap();
    let mut sum = [0f32; 16];
    for (xs, ws) in x.chunks_exact(16).zip(w.chunks_exact(16)) {
        for i in 0..16 {
            let c = xs[i].clamp(0, QA as i16) as i32;
            sum[i] += (c * c) as f32 * ws[i];
        }
    }
    sum.iter().sum()
}

#[derive(Clone)]
#[repr(C, align(64))]
pub struct Accumulator {
    pub v: [[i16; HIDDEN]; 2],
}

impl Default for Accumulator {
    fn default() -> Self {
        Accumulator { v: [[0; HIDDEN]; 2] }
    }
}
