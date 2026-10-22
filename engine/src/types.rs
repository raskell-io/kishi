//! Squares, pieces, moves and the bit tricks shared by everything else.
//!
//! Squares count from a1 = 0 to h8 = 63. A piece is `color * 6 + kind`.

pub type Bitboard = u64;

pub const WHITE: usize = 0;
pub const BLACK: usize = 1;

pub const PAWN: usize = 0;
pub const KNIGHT: usize = 1;
pub const BISHOP: usize = 2;
pub const ROOK: usize = 3;
pub const QUEEN: usize = 4;
pub const KING: usize = 5;

/// Marks an empty square in the mailbox.
pub const NO_PIECE: u8 = 12;
/// Marks "no en passant square".
pub const NO_SQ: u8 = 64;

pub const FILE_A: Bitboard = 0x0101_0101_0101_0101;
pub const FILE_H: Bitboard = FILE_A << 7;
pub const RANK_1: Bitboard = 0xff;
pub const RANK_2: Bitboard = RANK_1 << 8;
pub const RANK_4: Bitboard = RANK_1 << 24;
pub const RANK_5: Bitboard = RANK_1 << 32;
pub const RANK_7: Bitboard = RANK_1 << 48;
pub const RANK_8: Bitboard = RANK_1 << 56;

#[inline(always)]
pub const fn piece(color: usize, kind: usize) -> u8 {
    (color * 6 + kind) as u8
}

#[inline(always)]
pub const fn kind_of(piece: u8) -> usize {
    piece as usize % 6
}

#[inline(always)]
pub const fn color_of(piece: u8) -> usize {
    piece as usize / 6
}

#[inline(always)]
pub const fn bit(sq: usize) -> Bitboard {
    1 << sq
}

#[inline(always)]
pub const fn file_of(sq: usize) -> usize {
    sq & 7
}

#[inline(always)]
pub const fn rank_of(sq: usize) -> usize {
    sq >> 3
}

/// Index of the lowest set bit. The board must not be empty.
#[inline(always)]
pub fn lsb(b: Bitboard) -> usize {
    b.trailing_zeros() as usize
}

/// Iterates the squares of a bitboard, lowest first.
pub struct Squares(pub Bitboard);

impl Iterator for Squares {
    type Item = usize;
    #[inline(always)]
    fn next(&mut self) -> Option<usize> {
        if self.0 == 0 {
            None
        } else {
            let sq = lsb(self.0);
            self.0 &= self.0 - 1;
            Some(sq)
        }
    }
}

pub fn square_name(sq: usize) -> String {
    let f = (b'a' + file_of(sq) as u8) as char;
    let r = (b'1' + rank_of(sq) as u8) as char;
    format!("{f}{r}")
}

pub fn parse_square(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.len() != 2 || !(b'a'..=b'h').contains(&b[0]) || !(b'1'..=b'8').contains(&b[1]) {
        return None;
    }
    Some(((b[1] - b'1') * 8 + (b[0] - b'a')) as usize)
}

/// A move in 16 bits: from (6), to (6), flag (4). Castling is the king's move.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Hash)]
pub struct Move(pub u16);

pub const QUIET: u16 = 0;
pub const DOUBLE_PUSH: u16 = 1;
pub const KING_CASTLE: u16 = 2;
pub const QUEEN_CASTLE: u16 = 3;
pub const CAPTURE: u16 = 4;
pub const EN_PASSANT: u16 = 5;
/// Promotion flags are 8 + (kind - KNIGHT), plus 4 for a capture.
pub const PROMO: u16 = 8;

impl Move {
    pub const NONE: Move = Move(0);
    pub const NULL: Move = Move(65);

    #[inline(always)]
    pub const fn new(from: usize, to: usize, flag: u16) -> Move {
        Move(from as u16 | (to as u16) << 6 | flag << 12)
    }
    #[inline(always)]
    pub const fn from(self) -> usize {
        (self.0 & 63) as usize
    }
    #[inline(always)]
    pub const fn to(self) -> usize {
        (self.0 >> 6 & 63) as usize
    }
    #[inline(always)]
    pub const fn flag(self) -> u16 {
        self.0 >> 12
    }
    #[inline(always)]
    pub const fn is_capture(self) -> bool {
        self.flag() & CAPTURE != 0
    }
    #[inline(always)]
    pub const fn is_promotion(self) -> bool {
        self.flag() & PROMO != 0
    }
    /// Neither a capture nor a promotion.
    #[inline(always)]
    pub const fn is_quiet(self) -> bool {
        self.flag() & (CAPTURE | PROMO) == 0
    }
    #[inline(always)]
    pub const fn is_castle(self) -> bool {
        self.flag() == KING_CASTLE || self.flag() == QUEEN_CASTLE
    }
    #[inline(always)]
    pub const fn promotion(self) -> usize {
        (self.flag() & 3) as usize + KNIGHT
    }

    pub fn uci(self) -> String {
        if self == Move::NONE {
            return "0000".into();
        }
        let mut s = square_name(self.from()) + &square_name(self.to());
        if self.is_promotion() {
            s.push(b"nbrq"[self.promotion() - KNIGHT] as char);
        }
        s
    }
}

/// A fixed-capacity move list; no position has more than 218 legal moves.
pub struct MoveList {
    pub moves: [Move; 256],
    pub len: usize,
}

impl MoveList {
    #[inline(always)]
    pub fn new() -> Self {
        MoveList { moves: [Move::NONE; 256], len: 0 }
    }
    #[inline(always)]
    pub fn push(&mut self, m: Move) {
        self.moves[self.len] = m;
        self.len += 1;
    }
    #[inline(always)]
    pub fn as_slice(&self) -> &[Move] {
        &self.moves[..self.len]
    }
}

impl Default for MoveList {
    fn default() -> Self {
        Self::new()
    }
}

/// Deterministic xorshift generator for hashing keys, magic search and self-play.
#[derive(Clone)]
pub struct Rng(pub u64);

impl Rng {
    #[inline]
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// Uniform in 0..n.
    pub fn below(&mut self, n: usize) -> usize {
        ((self.next() >> 32) * n as u64 >> 32) as usize
    }
    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}
