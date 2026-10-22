//! The board: bitboards plus a mailbox, FEN, making moves and SAN.
//!
//! Positions are small and `Copy`; making a move returns a new one, so the
//! search never needs to unmake.

use crate::attacks::*;
use crate::types::*;

pub const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

pub const WK_CASTLE: u8 = 1;
pub const WQ_CASTLE: u8 = 2;
pub const BK_CASTLE: u8 = 4;
pub const BQ_CASTLE: u8 = 8;

struct Keys {
    pieces: [[u64; 64]; 12],
    castling: [u64; 16],
    ep_file: [u64; 8],
    black: u64,
}

const fn make_keys() -> Keys {
    let mut s: u64 = 0x2545_f491_4f6c_dd1d;
    let mut keys = Keys { pieces: [[0; 64]; 12], castling: [0; 16], ep_file: [0; 8], black: 0 };
    macro_rules! next {
        () => {{
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        }};
    }
    let mut p = 0;
    while p < 12 {
        let mut sq = 0;
        while sq < 64 {
            keys.pieces[p][sq] = next!();
            sq += 1;
        }
        p += 1;
    }
    let mut i = 0;
    while i < 16 {
        keys.castling[i] = next!();
        i += 1;
    }
    let mut f = 0;
    while f < 8 {
        keys.ep_file[f] = next!();
        f += 1;
    }
    keys.black = next!();
    keys
}

static KEYS: Keys = make_keys();

/// Rights that survive a move touching each square.
const fn castle_masks() -> [u8; 64] {
    let mut m = [15u8; 64];
    m[0] = 15 & !WQ_CASTLE;
    m[4] = 15 & !(WK_CASTLE | WQ_CASTLE);
    m[7] = 15 & !WK_CASTLE;
    m[56] = 15 & !BQ_CASTLE;
    m[60] = 15 & !(BK_CASTLE | BQ_CASTLE);
    m[63] = 15 & !BK_CASTLE;
    m
}
static CASTLE_MASK: [u8; 64] = castle_masks();

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub by_kind: [Bitboard; 6],
    pub by_color: [Bitboard; 2],
    pub board: [u8; 64],
    pub stm: usize,
    pub castling: u8,
    /// Set only when a pawn of the side to move could actually capture there.
    pub ep: u8,
    pub halfmove: u16,
    pub fullmove: u16,
    pub hash: u64,
    /// Hash of the pawns alone, for the evaluation's correction history.
    pub pawn_hash: u64,
}

impl Position {
    pub fn startpos() -> Position {
        Position::from_fen(START_FEN).unwrap()
    }

    fn empty() -> Position {
        Position {
            by_kind: [0; 6],
            by_color: [0; 2],
            board: [NO_PIECE; 64],
            stm: WHITE,
            castling: 0,
            ep: NO_SQ,
            halfmove: 0,
            fullmove: 1,
            hash: 0,
            pawn_hash: 0,
        }
    }

    #[inline(always)]
    pub fn occupied(&self) -> Bitboard {
        self.by_color[0] | self.by_color[1]
    }

    #[inline(always)]
    pub fn pieces(&self, color: usize, kind: usize) -> Bitboard {
        self.by_color[color] & self.by_kind[kind]
    }

    #[inline(always)]
    pub fn king_square(&self, color: usize) -> usize {
        lsb(self.pieces(color, KING))
    }

    #[inline(always)]
    fn put(&mut self, sq: usize, p: u8) {
        self.board[sq] = p;
        self.by_kind[kind_of(p)] |= bit(sq);
        self.by_color[color_of(p)] |= bit(sq);
        self.hash ^= KEYS.pieces[p as usize][sq];
        if kind_of(p) == PAWN {
            self.pawn_hash ^= KEYS.pieces[p as usize][sq];
        }
    }

    #[inline(always)]
    fn remove(&mut self, sq: usize) {
        let p = self.board[sq];
        self.board[sq] = NO_PIECE;
        self.by_kind[kind_of(p)] ^= bit(sq);
        self.by_color[color_of(p)] ^= bit(sq);
        self.hash ^= KEYS.pieces[p as usize][sq];
        if kind_of(p) == PAWN {
            self.pawn_hash ^= KEYS.pieces[p as usize][sq];
        }
    }

    pub fn from_fen(fen: &str) -> Result<Position, String> {
        let mut pos = Position::empty();
        let mut parts = fen.split_whitespace();
        let placement = parts.next().ok_or("empty FEN")?;
        let (mut rank, mut file) = (7i32, 0i32);
        for c in placement.chars() {
            match c {
                '/' => {
                    rank -= 1;
                    file = 0;
                }
                '1'..='8' => file += c as i32 - '0' as i32,
                _ => {
                    let kind = "pnbrqk".find(c.to_ascii_lowercase()).ok_or(format!("bad piece '{c}'"))?;
                    let color = if c.is_ascii_uppercase() { WHITE } else { BLACK };
                    if !(0..8).contains(&rank) || !(0..8).contains(&file) {
                        return Err("bad placement".into());
                    }
                    pos.put((rank * 8 + file) as usize, piece(color, kind));
                    file += 1;
                }
            }
        }
        if pos.pieces(WHITE, KING).count_ones() != 1 || pos.pieces(BLACK, KING).count_ones() != 1 {
            return Err("each side needs one king".into());
        }
        pos.stm = match parts.next().unwrap_or("w") {
            "w" => WHITE,
            "b" => BLACK,
            s => return Err(format!("bad side to move '{s}'")),
        };
        for c in parts.next().unwrap_or("-").chars() {
            pos.castling |= match c {
                'K' => WK_CASTLE,
                'Q' => WQ_CASTLE,
                'k' => BK_CASTLE,
                'q' => BQ_CASTLE,
                _ => 0,
            };
        }
        // Drop rights the pieces no longer support.
        let rights = [(WK_CASTLE, 4, 7, WHITE), (WQ_CASTLE, 4, 0, WHITE), (BK_CASTLE, 60, 63, BLACK), (BQ_CASTLE, 60, 56, BLACK)];
        for (flag, k, r, c) in rights {
            if pos.board[k] != piece(c, KING) || pos.board[r] != piece(c, ROOK) {
                pos.castling &= !flag;
            }
        }
        if let Some(sq) = parse_square(parts.next().unwrap_or("-")) {
            pos.set_ep(sq);
        }
        pos.halfmove = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        pos.fullmove = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1).max(1);
        pos.hash ^= KEYS.castling[pos.castling as usize];
        if pos.stm == BLACK {
            pos.hash ^= KEYS.black;
        }
        if pos.attackers_to(pos.king_square(pos.stm ^ 1), pos.occupied()) & pos.by_color[pos.stm] != 0 {
            return Err("the side not to move is in check".into());
        }
        Ok(pos)
    }

    /// Records an en passant square if the side to move has a pawn able to use it.
    fn set_ep(&mut self, sq: usize) {
        if PAWN_ATTACKS[self.stm ^ 1][sq] & self.pieces(self.stm, PAWN) != 0 {
            self.ep = sq as u8;
            self.hash ^= KEYS.ep_file[file_of(sq)];
        }
    }

    pub fn fen(&self) -> String {
        let mut s = String::new();
        for rank in (0..8).rev() {
            let mut empty = 0;
            for file in 0..8 {
                let p = self.board[rank * 8 + file];
                if p == NO_PIECE {
                    empty += 1;
                    continue;
                }
                if empty > 0 {
                    s.push((b'0' + empty) as char);
                    empty = 0;
                }
                let c = b"pnbrqk"[kind_of(p)] as char;
                s.push(if color_of(p) == WHITE { c.to_ascii_uppercase() } else { c });
            }
            if empty > 0 {
                s.push((b'0' + empty) as char);
            }
            if rank > 0 {
                s.push('/');
            }
        }
        s.push_str(if self.stm == WHITE { " w " } else { " b " });
        if self.castling == 0 {
            s.push('-');
        }
        for (flag, c) in [(WK_CASTLE, 'K'), (WQ_CASTLE, 'Q'), (BK_CASTLE, 'k'), (BQ_CASTLE, 'q')] {
            if self.castling & flag != 0 {
                s.push(c);
            }
        }
        let ep = if self.ep == NO_SQ { "-".to_string() } else { square_name(self.ep as usize) };
        format!("{s} {ep} {} {}", self.halfmove, self.fullmove)
    }

    /// Every piece of either color attacking `sq` given the occupancy `occ`.
    #[inline]
    pub fn attackers_to(&self, sq: usize, occ: Bitboard) -> Bitboard {
        (PAWN_ATTACKS[BLACK][sq] & self.pieces(WHITE, PAWN))
            | (PAWN_ATTACKS[WHITE][sq] & self.pieces(BLACK, PAWN))
            | (KNIGHT_ATTACKS[sq] & self.by_kind[KNIGHT])
            | (KING_ATTACKS[sq] & self.by_kind[KING])
            | (bishop_attacks(sq, occ) & (self.by_kind[BISHOP] | self.by_kind[QUEEN]))
            | (rook_attacks(sq, occ) & (self.by_kind[ROOK] | self.by_kind[QUEEN]))
    }

    #[inline]
    pub fn is_attacked(&self, sq: usize, by: usize) -> bool {
        let occ = self.occupied();
        let them = self.by_color[by];
        PAWN_ATTACKS[by ^ 1][sq] & self.by_kind[PAWN] & them != 0
            || KNIGHT_ATTACKS[sq] & self.by_kind[KNIGHT] & them != 0
            || KING_ATTACKS[sq] & self.by_kind[KING] & them != 0
            || bishop_attacks(sq, occ) & (self.by_kind[BISHOP] | self.by_kind[QUEEN]) & them != 0
            || rook_attacks(sq, occ) & (self.by_kind[ROOK] | self.by_kind[QUEEN]) & them != 0
    }

    #[inline]
    pub fn in_check(&self) -> bool {
        self.is_attacked(self.king_square(self.stm), self.stm ^ 1)
    }

    /// After a move: whether the side that just moved left its king attacked.
    #[inline]
    pub fn left_king_in_check(&self) -> bool {
        self.is_attacked(self.king_square(self.stm ^ 1), self.stm)
    }

    /// The piece a move captures, or `NO_PIECE`.
    #[inline]
    pub fn captured(&self, m: Move) -> u8 {
        if m.flag() == EN_PASSANT { piece(self.stm ^ 1, PAWN) } else { self.board[m.to()] }
    }

    /// Plays a pseudo-legal move. Check legality with `left_king_in_check`.
    pub fn make(&self, m: Move) -> Position {
        let mut p = *self;
        let (from, to, flag) = (m.from(), m.to(), m.flag());
        let us = self.stm;
        let moving = self.board[from];

        p.hash ^= KEYS.castling[p.castling as usize];
        if p.ep != NO_SQ {
            p.hash ^= KEYS.ep_file[file_of(p.ep as usize)];
            p.ep = NO_SQ;
        }
        p.halfmove += 1;

        if flag == EN_PASSANT {
            p.remove(to ^ 8);
        } else if m.is_capture() {
            p.remove(to);
        }
        if kind_of(moving) == PAWN || m.is_capture() {
            p.halfmove = 0;
        }
        p.remove(from);
        p.put(to, if m.is_promotion() { piece(us, m.promotion()) } else { moving });

        match flag {
            KING_CASTLE => {
                p.remove(to + 1);
                p.put(to - 1, piece(us, ROOK));
            }
            QUEEN_CASTLE => {
                p.remove(to - 2);
                p.put(to + 1, piece(us, ROOK));
            }
            _ => {}
        }

        p.castling &= CASTLE_MASK[from] & CASTLE_MASK[to];
        p.hash ^= KEYS.castling[p.castling as usize];
        p.stm ^= 1;
        p.hash ^= KEYS.black;
        if flag == DOUBLE_PUSH {
            p.set_ep((from + to) / 2);
        }
        if us == BLACK {
            p.fullmove += 1;
        }
        p
    }

    /// Passes the turn, for null-move pruning.
    pub fn make_null(&self) -> Position {
        let mut p = *self;
        if p.ep != NO_SQ {
            p.hash ^= KEYS.ep_file[file_of(p.ep as usize)];
            p.ep = NO_SQ;
        }
        p.stm ^= 1;
        p.hash ^= KEYS.black;
        p.halfmove += 1;
        p
    }

    pub fn has_non_pawn_material(&self, color: usize) -> bool {
        self.by_color[color] & !(self.by_kind[PAWN] | self.by_kind[KING]) != 0
    }

    /// Neither side can mate: bare kings, or a lone minor piece.
    pub fn insufficient_material(&self) -> bool {
        let heavy = self.by_kind[PAWN] | self.by_kind[ROOK] | self.by_kind[QUEEN];
        heavy == 0 && (self.by_kind[KNIGHT] | self.by_kind[BISHOP]).count_ones() <= 1
    }

    pub fn legal_moves(&self) -> Vec<Move> {
        let mut list = MoveList::new();
        self.generate(&mut list, false);
        list.as_slice().iter().copied().filter(|&m| !self.make(m).left_king_in_check()).collect()
    }

    pub fn parse_uci(&self, s: &str) -> Option<Move> {
        self.legal_moves().into_iter().find(|m| m.uci() == s)
    }

    /// Standard algebraic notation, with + or # for check and mate.
    pub fn san(&self, m: Move) -> String {
        let (from, to) = (m.from(), m.to());
        let kind = kind_of(self.board[from]);
        let mut s = String::new();
        if m.flag() == KING_CASTLE {
            s.push_str("O-O");
        } else if m.flag() == QUEEN_CASTLE {
            s.push_str("O-O-O");
        } else if kind == PAWN {
            if m.is_capture() {
                s.push((b'a' + file_of(from) as u8) as char);
                s.push('x');
            }
            s.push_str(&square_name(to));
            if m.is_promotion() {
                s.push('=');
                s.push(b"NBRQ"[m.promotion() - KNIGHT] as char);
            }
        } else {
            s.push(b"PNBRQK"[kind] as char);
            let rivals: Vec<Move> = self
                .legal_moves()
                .into_iter()
                .filter(|o| o.to() == to && o.from() != from && kind_of(self.board[o.from()]) == kind)
                .collect();
            if !rivals.is_empty() {
                let same_file = rivals.iter().any(|o| file_of(o.from()) == file_of(from));
                let same_rank = rivals.iter().any(|o| rank_of(o.from()) == rank_of(from));
                if !same_file {
                    s.push((b'a' + file_of(from) as u8) as char);
                } else if !same_rank {
                    s.push((b'1' + rank_of(from) as u8) as char);
                } else {
                    s.push_str(&square_name(from));
                }
            }
            if m.is_capture() {
                s.push('x');
            }
            s.push_str(&square_name(to));
        }
        let next = self.make(m);
        if next.in_check() {
            s.push(if next.legal_moves().is_empty() { '#' } else { '+' });
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fen_round_trip() {
        for fen in [
            START_FEN,
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        ] {
            assert_eq!(Position::from_fen(fen).unwrap().fen(), fen);
        }
    }

    #[test]
    fn hash_is_incremental() {
        let mut pos = Position::startpos();
        let mut rng = Rng(7);
        for _ in 0..2000 {
            let moves = pos.legal_moves();
            if moves.is_empty() || pos.halfmove >= 100 {
                pos = Position::startpos();
                continue;
            }
            pos = pos.make(moves[rng.below(moves.len())]);
            let fresh = Position::from_fen(&pos.fen()).unwrap();
            assert_eq!(pos.hash, fresh.hash, "{}", pos.fen());
            assert_eq!(pos.pawn_hash, fresh.pawn_hash, "{}", pos.fen());
        }
    }

    #[test]
    fn san() {
        let pos = Position::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1").unwrap();
        let san = |u: &str| pos.san(pos.parse_uci(u).unwrap());
        assert_eq!(san("e1g1"), "O-O");
        assert_eq!(san("e5f7"), "Nxf7");
        assert_eq!(san("d5e6"), "dxe6");
        assert_eq!(san("c3b1"), "Nb1");
        assert_eq!(san("e2a6"), "Bxa6");
    }
}
