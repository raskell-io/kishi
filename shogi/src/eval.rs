//! A hand-written shogi evaluation: material (pieces in hand a little above
//! their board value, since a drop can land anywhere), king shelter, pressure
//! on the enemy king, and mobility of the major pieces.

use crate::position::*;

/// Board values by kind, a pawn being 100.
pub const VALUE: [i32; 15] = [0, 100, 320, 380, 520, 580, 850, 980, 0, 560, 560, 560, 570, 1100, 1300];
/// In-hand values by kind.
const HAND: [i32; 8] = [0, 115, 340, 420, 580, 640, 950, 1100];

#[inline]
fn distance(a: usize, b: usize) -> i32 {
    (row(a) as i32 - row(b) as i32).abs().max((col(a) as i32 - col(b) as i32).abs())
}

/// How many squares a sliding piece reaches, for mobility.
fn reach(pos: &Position, sq: usize, dirs: &[(i32, i32)]) -> i32 {
    let mut n = 0;
    for &(dr, dc) in dirs {
        let (mut r, mut c) = (row(sq) as i32 + dr, col(sq) as i32 + dc);
        while (0..9).contains(&r) && (0..9).contains(&c) {
            n += 1;
            if pos.board[(r * 9 + c) as usize] != EMPTY {
                break;
            }
            r += dr;
            c += dc;
        }
    }
    n
}

const DIAG: [(i32, i32); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];
const ORTH: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

/// Centipawns from the side to move's point of view.
pub fn evaluate(pos: &Position) -> i32 {
    let mut score = [0i32; 2];
    let kings = [pos.kings[SENTE] as usize, pos.kings[GOTE] as usize];
    for side in [SENTE, GOTE] {
        for k in PAWN..=ROOK {
            score[side] += HAND[k as usize] * pos.hand[side][k as usize] as i32;
        }
    }
    for sq in 0..81 {
        let p = pos.board[sq];
        if p == EMPTY {
            continue;
        }
        let (side, k) = (color(p), kind(p));
        let own_king = kings[side];
        let their_king = kings[side ^ 1];
        let s = &mut score[side];
        if k == KING {
            // Keep the king home until the board empties.
            *s -= 18 * (rank_from(side, sq) as i32).saturating_sub(1);
            continue;
        }
        *s += VALUE[k as usize];
        let far = rank_from(side, sq) as i32;
        match k {
            PAWN => *s += 4 * far,
            SILVER | GOLD => {
                // Defenders near their own king, attackers near the enemy's.
                if distance(sq, own_king) <= 2 {
                    *s += 24;
                }
            }
            BISHOP | HORSE => *s += 4 * reach(pos, sq, &DIAG),
            ROOK | DRAGON => *s += 4 * reach(pos, sq, &ORTH),
            _ => {}
        }
        // Pressure: every piece near the enemy king counts, more the closer it is.
        let d = distance(sq, their_king);
        if d <= 3 {
            let weight = match k {
                PAWN => 6,
                DRAGON | HORSE | TOKIN | PRO_SILVER | PRO_KNIGHT | PRO_LANCE => 28,
                _ => 16,
            };
            *s += weight * (4 - d);
        }
    }
    let us = pos.stm;
    score[us] - score[us ^ 1] + 20
}
