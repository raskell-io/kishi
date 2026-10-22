//! Pseudo-legal move generation. Moves that leave the king in check are
//! filtered by the caller after making them.

use crate::attacks::*;
use crate::position::*;
use crate::types::*;

impl Position {
    /// Appends pseudo-legal moves. With `noisy_only`, just captures and queen promotions.
    pub fn generate(&self, list: &mut MoveList, noisy_only: bool) {
        let us = self.stm;
        let them = us ^ 1;
        let occ = self.occupied();
        let enemies = self.by_color[them];
        let targets = if noisy_only { enemies } else { !self.by_color[us] };

        self.pawn_moves(list, noisy_only);

        for kind in KNIGHT..=KING {
            for from in Squares(self.pieces(us, kind)) {
                let attacks = match kind {
                    KNIGHT => KNIGHT_ATTACKS[from],
                    BISHOP => bishop_attacks(from, occ),
                    ROOK => rook_attacks(from, occ),
                    QUEEN => queen_attacks(from, occ),
                    _ => KING_ATTACKS[from],
                };
                let moves = attacks & targets;
                for to in Squares(moves & enemies) {
                    list.push(Move::new(from, to, CAPTURE));
                }
                for to in Squares(moves & !occ) {
                    list.push(Move::new(from, to, QUIET));
                }
            }
        }

        if !noisy_only {
            self.castling_moves(list);
        }
    }

    fn pawn_moves(&self, list: &mut MoveList, noisy_only: bool) {
        let us = self.stm;
        let pawns = self.pieces(us, PAWN);
        let empty = !self.occupied();
        let enemies = self.by_color[us ^ 1];
        let (up, promo_rank, third_rank): (i32, Bitboard, Bitboard) =
            if us == WHITE { (8, RANK_8, RANK_1 << 16) } else { (-8, RANK_1, RANK_1 << 40) };
        let shift = |b: Bitboard, d: i32| if d > 0 { b << d } else { b >> -d };

        let single = shift(pawns, up) & empty;
        let double = shift(single & third_rank, up) & empty;
        let west = shift(pawns & !FILE_A, up - 1) & enemies;
        let east = shift(pawns & !FILE_H, up + 1) & enemies;

        let push_promos = |list: &mut MoveList, from: usize, to: usize, capture: u16| {
            list.push(Move::new(from, to, PROMO | capture | 3));
            if !noisy_only {
                for k in [2, 1, 0] {
                    list.push(Move::new(from, to, PROMO | capture | k));
                }
            }
        };

        for to in Squares(single & promo_rank) {
            push_promos(list, (to as i32 - up) as usize, to, 0);
        }
        for (targets, d) in [(west, up - 1), (east, up + 1)] {
            for to in Squares(targets & promo_rank) {
                push_promos(list, (to as i32 - d) as usize, to, CAPTURE);
            }
            for to in Squares(targets & !promo_rank) {
                list.push(Move::new((to as i32 - d) as usize, to, CAPTURE));
            }
        }
        if self.ep != NO_SQ {
            let ep = self.ep as usize;
            for from in Squares(PAWN_ATTACKS[us ^ 1][ep] & pawns) {
                list.push(Move::new(from, ep, EN_PASSANT));
            }
        }
        if noisy_only {
            return;
        }
        for to in Squares(single & !promo_rank) {
            list.push(Move::new((to as i32 - up) as usize, to, QUIET));
        }
        for to in Squares(double) {
            list.push(Move::new((to as i32 - 2 * up) as usize, to, DOUBLE_PUSH));
        }
    }

    fn castling_moves(&self, list: &mut MoveList) {
        let us = self.stm;
        let (king_side, queen_side, base) =
            if us == WHITE { (WK_CASTLE, WQ_CASTLE, 0) } else { (BK_CASTLE, BQ_CASTLE, 56) };
        if self.castling & (king_side | queen_side) == 0 {
            return;
        }
        let occ = self.occupied();
        let them = us ^ 1;
        if self.is_attacked(base + 4, them) {
            return;
        }
        if self.castling & king_side != 0
            && occ & (bit(base + 5) | bit(base + 6)) == 0
            && !self.is_attacked(base + 5, them)
        {
            list.push(Move::new(base + 4, base + 6, KING_CASTLE));
        }
        if self.castling & queen_side != 0
            && occ & (bit(base + 1) | bit(base + 2) | bit(base + 3)) == 0
            && !self.is_attacked(base + 3, them)
        {
            list.push(Move::new(base + 4, base + 2, QUEEN_CASTLE));
        }
    }
}

/// Counts leaf nodes of the legal move tree; the standard move generator test.
pub fn perft(pos: &Position, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let mut list = MoveList::new();
    pos.generate(&mut list, false);
    let mut nodes = 0;
    for &m in list.as_slice() {
        let next = pos.make(m);
        if next.left_king_in_check() {
            continue;
        }
        nodes += if depth == 1 { 1 } else { perft(&next, depth - 1) };
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perft_suite() {
        let cases: &[(&str, &[u64])] = &[
            (START_FEN, &[20, 400, 8902, 197_281, 4_865_609]),
            ("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1", &[48, 2039, 97_862, 4_085_603]),
            ("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1", &[14, 191, 2812, 43_238, 674_624]),
            ("r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1", &[6, 264, 9467, 422_333]),
            ("rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8", &[44, 1486, 62_379, 2_103_487]),
            ("r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10", &[46, 2079, 89_890, 3_894_594]),
        ];
        for (fen, counts) in cases {
            let pos = Position::from_fen(fen).unwrap();
            for (d, &expected) in counts.iter().enumerate() {
                assert_eq!(perft(&pos, d as u32 + 1), expected, "{fen} depth {}", d + 1);
            }
        }
    }

    #[test]
    fn noisy_moves_are_a_subset() {
        let pos = Position::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1").unwrap();
        let (mut all, mut noisy) = (MoveList::new(), MoveList::new());
        pos.generate(&mut all, false);
        pos.generate(&mut noisy, true);
        assert_eq!(noisy.len, 8);
        assert!(noisy.as_slice().iter().all(|m| all.as_slice().contains(m)));
    }
}
