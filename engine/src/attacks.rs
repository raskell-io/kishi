//! Attack tables: leapers computed at compile time, sliders by magic bitboards
//! found once at start-up with a fixed seed.

use crate::types::*;
use std::sync::OnceLock;

const fn leaper_table(deltas: &[(i32, i32)]) -> [Bitboard; 64] {
    let mut table = [0; 64];
    let mut sq = 0;
    while sq < 64 {
        let (f, r) = ((sq % 8) as i32, (sq / 8) as i32);
        let mut i = 0;
        while i < deltas.len() {
            let (nf, nr) = (f + deltas[i].0, r + deltas[i].1);
            if nf >= 0 && nf < 8 && nr >= 0 && nr < 8 {
                table[sq] |= 1 << (nr * 8 + nf);
            }
            i += 1;
        }
        sq += 1;
    }
    table
}

pub static KNIGHT_ATTACKS: [Bitboard; 64] =
    leaper_table(&[(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)]);
pub static KING_ATTACKS: [Bitboard; 64] =
    leaper_table(&[(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)]);
/// Squares a pawn of each color attacks from each square.
pub static PAWN_ATTACKS: [[Bitboard; 64]; 2] =
    [leaper_table(&[(-1, 1), (1, 1)]), leaper_table(&[(-1, -1), (1, -1)])];

const ROOK_DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const BISHOP_DIRS: [(i32, i32); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];

fn ray_attacks(sq: usize, occ: Bitboard, dirs: &[(i32, i32); 4]) -> Bitboard {
    let mut attacks = 0;
    for &(df, dr) in dirs {
        let (mut f, mut r) = ((sq % 8) as i32 + df, (sq / 8) as i32 + dr);
        while (0..8).contains(&f) && (0..8).contains(&r) {
            let b = 1u64 << (r * 8 + f);
            attacks |= b;
            if occ & b != 0 {
                break;
            }
            f += df;
            r += dr;
        }
    }
    attacks
}

/// The squares whose occupancy matters for a slider on `sq`: its rays minus the edges.
fn relevant_mask(sq: usize, dirs: &[(i32, i32); 4]) -> Bitboard {
    let mut mask = 0;
    for &(df, dr) in dirs {
        let (mut f, mut r) = ((sq % 8) as i32 + df, (sq / 8) as i32 + dr);
        while (0..8).contains(&(f + df)) && (0..8).contains(&(r + dr)) {
            mask |= 1u64 << (r * 8 + f);
            f += df;
            r += dr;
        }
    }
    mask
}

#[derive(Clone, Copy, Default)]
struct Magic {
    mask: Bitboard,
    magic: u64,
    shift: u32,
    offset: usize,
}

struct Sliders {
    rook: [Magic; 64],
    bishop: [Magic; 64],
    table: Vec<Bitboard>,
    /// Squares strictly between two squares on a shared line, else empty.
    between: Vec<[Bitboard; 64]>,
}

static SLIDERS: OnceLock<Sliders> = OnceLock::new();

fn find_magics(dirs: &[(i32, i32); 4], table: &mut Vec<Bitboard>, rng: &mut Rng) -> [Magic; 64] {
    let mut magics = [Magic::default(); 64];
    for (sq, entry) in magics.iter_mut().enumerate() {
        let mask = relevant_mask(sq, dirs);
        let bits = mask.count_ones();
        let size = 1usize << bits;
        // Every subset of the mask (Carry-Rippler) and its true attack set.
        let mut occs = Vec::with_capacity(size);
        let mut refs = Vec::with_capacity(size);
        let mut sub: Bitboard = 0;
        loop {
            occs.push(sub);
            refs.push(ray_attacks(sq, sub, dirs));
            sub = sub.wrapping_sub(mask) & mask;
            if sub == 0 {
                break;
            }
        }
        let offset = table.len();
        table.resize(offset + size, 0);
        let mut epoch = vec![0u32; size];
        let mut attempt = 0u32;
        loop {
            let magic = rng.next() & rng.next() & rng.next();
            if (mask.wrapping_mul(magic) >> 56).count_ones() < 6 {
                continue;
            }
            attempt += 1;
            let mut ok = true;
            for i in 0..size {
                let idx = (occs[i].wrapping_mul(magic) >> (64 - bits)) as usize;
                if epoch[idx] < attempt {
                    epoch[idx] = attempt;
                    table[offset + idx] = refs[i];
                } else if table[offset + idx] != refs[i] {
                    ok = false;
                    break;
                }
            }
            if ok {
                *entry = Magic { mask, magic, shift: 64 - bits, offset };
                break;
            }
        }
    }
    magics
}

fn sliders() -> &'static Sliders {
    SLIDERS.get_or_init(|| {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut table = Vec::with_capacity(107_648);
        let rook = find_magics(&ROOK_DIRS, &mut table, &mut rng);
        let bishop = find_magics(&BISHOP_DIRS, &mut table, &mut rng);
        let mut between = vec![[0; 64]; 64];
        for a in 0..64 {
            for b in 0..64 {
                for dirs in [&ROOK_DIRS, &BISHOP_DIRS] {
                    if ray_attacks(a, 0, dirs) & bit(b) != 0 {
                        between[a][b] = ray_attacks(a, bit(b), dirs) & ray_attacks(b, bit(a), dirs);
                    }
                }
            }
        }
        Sliders { rook, bishop, table, between }
    })
}

/// Builds the slider tables now rather than on first use.
pub fn init() {
    sliders();
}

#[inline(always)]
pub fn rook_attacks(sq: usize, occ: Bitboard) -> Bitboard {
    let s = sliders();
    let m = &s.rook[sq];
    unsafe { *s.table.get_unchecked(m.offset + ((occ & m.mask).wrapping_mul(m.magic) >> m.shift) as usize) }
}

#[inline(always)]
pub fn bishop_attacks(sq: usize, occ: Bitboard) -> Bitboard {
    let s = sliders();
    let m = &s.bishop[sq];
    unsafe { *s.table.get_unchecked(m.offset + ((occ & m.mask).wrapping_mul(m.magic) >> m.shift) as usize) }
}

#[inline(always)]
pub fn queen_attacks(sq: usize, occ: Bitboard) -> Bitboard {
    rook_attacks(sq, occ) | bishop_attacks(sq, occ)
}

#[inline(always)]
pub fn between(a: usize, b: usize) -> Bitboard {
    sliders().between[a][b]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sliders_match_ray_walk() {
        let mut rng = Rng(42);
        for _ in 0..20_000 {
            let occ = rng.next() & rng.next();
            let sq = rng.below(64);
            assert_eq!(rook_attacks(sq, occ), ray_attacks(sq, occ, &ROOK_DIRS));
            assert_eq!(bishop_attacks(sq, occ), ray_attacks(sq, occ, &BISHOP_DIRS));
        }
    }
}
