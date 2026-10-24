//! The shogi board: a 9x9 mailbox, both hands, SFEN, making moves, and the
//! rules that make a move legal.
//!
//! Squares count from the top-left as sente sees the board: index = row * 9 + col,
//! with row 0 the rank 'a' (gote's back rank) and col 0 the 9-file. Sente (black,
//! who moves first) sits at the bottom and moves up.

pub const SENTE: usize = 0;
pub const GOTE: usize = 1;

pub const PAWN: u8 = 1;
pub const LANCE: u8 = 2;
pub const KNIGHT: u8 = 3;
pub const SILVER: u8 = 4;
pub const GOLD: u8 = 5;
pub const BISHOP: u8 = 6;
pub const ROOK: u8 = 7;
pub const KING: u8 = 8;
pub const TOKIN: u8 = 9;
pub const PRO_LANCE: u8 = 10;
pub const PRO_KNIGHT: u8 = 11;
pub const PRO_SILVER: u8 = 12;
pub const HORSE: u8 = 13;
pub const DRAGON: u8 = 14;

/// A piece is its kind, plus 16 when it belongs to gote; 0 is an empty square.
pub const EMPTY: u8 = 0;
const GOTE_BIT: u8 = 16;

pub const START_SFEN: &str = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";

#[inline(always)]
pub const fn kind(p: u8) -> u8 {
    p & 15
}
#[inline(always)]
pub const fn color(p: u8) -> usize {
    (p >> 4) as usize
}
#[inline(always)]
pub const fn make(c: usize, k: u8) -> u8 {
    k | if c == GOTE { GOTE_BIT } else { 0 }
}
#[inline(always)]
pub const fn can_promote(k: u8) -> bool {
    matches!(k, PAWN | LANCE | KNIGHT | SILVER | BISHOP | ROOK)
}
#[inline(always)]
pub const fn promoted(k: u8) -> u8 {
    match k {
        BISHOP => HORSE,
        ROOK => DRAGON,
        _ => k + 8,
    }
}
/// The kind a captured piece becomes in the hand.
#[inline(always)]
pub const fn unpromoted(k: u8) -> u8 {
    match k {
        TOKIN => PAWN,
        PRO_LANCE => LANCE,
        PRO_KNIGHT => KNIGHT,
        PRO_SILVER => SILVER,
        HORSE => BISHOP,
        DRAGON => ROOK,
        _ => k,
    }
}

#[inline(always)]
pub const fn row(sq: usize) -> usize {
    sq / 9
}
#[inline(always)]
pub const fn col(sq: usize) -> usize {
    sq % 9
}

/// Rows counted from the side's own back rank: 0 is where it starts, 8 the far side.
#[inline(always)]
pub const fn rank_from(c: usize, sq: usize) -> usize {
    if c == SENTE { 8 - row(sq) } else { row(sq) }
}

#[inline(always)]
fn in_zone(c: usize, sq: usize) -> bool {
    rank_from(c, sq) >= 6
}

/// A move in 16 bits: to (7), from (7; 81 + kind for a drop), promote (1).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Hash)]
pub struct Move(pub u16);

impl Move {
    pub const NONE: Move = Move(0);
    pub const NULL: Move = Move(1 | 1 << 7);

    #[inline(always)]
    pub const fn normal(from: usize, to: usize, promote: bool) -> Move {
        Move(to as u16 | (from as u16) << 7 | (promote as u16) << 14)
    }
    #[inline(always)]
    pub const fn drop(k: u8, to: usize) -> Move {
        Move(to as u16 | (81 + k as u16) << 7)
    }
    #[inline(always)]
    pub const fn to(self) -> usize {
        (self.0 & 127) as usize
    }
    #[inline(always)]
    pub const fn from(self) -> usize {
        (self.0 >> 7 & 127) as usize
    }
    #[inline(always)]
    pub const fn is_drop(self) -> bool {
        self.from() > 80
    }
    #[inline(always)]
    pub const fn drop_kind(self) -> u8 {
        (self.from() - 81) as u8
    }
    #[inline(always)]
    pub const fn is_promotion(self) -> bool {
        self.0 >> 14 & 1 == 1
    }

    pub fn usi(self) -> String {
        if self.is_drop() {
            format!("{}*{}", HAND_LETTERS[self.drop_kind() as usize] as char, square_name(self.to()))
        } else {
            let p = if self.is_promotion() { "+" } else { "" };
            format!("{}{}{p}", square_name(self.from()), square_name(self.to()))
        }
    }
}

const HAND_LETTERS: &[u8; 8] = b" PLNSGBR";
/// Letters of every kind, for SFEN (promoted kinds take a '+').
const SFEN_LETTERS: &[u8; 9] = b" PLNSGBRK";

pub fn square_name(sq: usize) -> String {
    format!("{}{}", 9 - col(sq), (b'a' + row(sq) as u8) as char)
}

pub fn parse_square(s: &[u8]) -> Option<usize> {
    if s.len() != 2 || !(b'1'..=b'9').contains(&s[0]) || !(b'a'..=b'i').contains(&s[1]) {
        return None;
    }
    Some((s[1] - b'a') as usize * 9 + (9 - (s[0] - b'0') as usize))
}

// Steps as (row, col) for sente; gote's are mirrored.
const N: (i32, i32) = (-1, 0);
const S: (i32, i32) = (1, 0);
const E: (i32, i32) = (0, 1);
const W: (i32, i32) = (0, -1);
const NE: (i32, i32) = (-1, 1);
const NW: (i32, i32) = (-1, -1);
const SE: (i32, i32) = (1, 1);
const SW: (i32, i32) = (1, -1);
const GOLD_STEPS: &[(i32, i32)] = &[N, NE, NW, E, W, S];
const KING_STEPS: &[(i32, i32)] = &[N, NE, NW, E, W, S, SE, SW];
const DIAGONALS: &[(i32, i32)] = &[NE, NW, SE, SW];
const ORTHOGONALS: &[(i32, i32)] = &[N, S, E, W];

/// (single steps, sliding directions) for a kind, from sente's side.
fn movement(k: u8) -> (&'static [(i32, i32)], &'static [(i32, i32)]) {
    match k {
        PAWN => (&[N], &[]),
        LANCE => (&[], &[N]),
        KNIGHT => (&[(-2, 1), (-2, -1)], &[]),
        SILVER => (&[N, NE, NW, SE, SW], &[]),
        GOLD | TOKIN | PRO_LANCE | PRO_KNIGHT | PRO_SILVER => (GOLD_STEPS, &[]),
        BISHOP => (&[], DIAGONALS),
        ROOK => (&[], ORTHOGONALS),
        KING => (KING_STEPS, &[]),
        HORSE => (ORTHOGONALS, DIAGONALS),
        DRAGON => (DIAGONALS, ORTHOGONALS),
        _ => (&[], &[]),
    }
}

#[inline(always)]
fn step(sq: usize, c: usize, (dr, dc): (i32, i32)) -> Option<usize> {
    let dr = if c == SENTE { dr } else { -dr };
    let dc = if c == SENTE { dc } else { -dc };
    let (r, c2) = (row(sq) as i32 + dr, col(sq) as i32 + dc);
    ((0..9).contains(&r) && (0..9).contains(&c2)).then(|| (r * 9 + c2) as usize)
}

struct Keys {
    board: [[u64; 81]; 32],
    hand: [[[u64; 19]; 8]; 2],
    gote: u64,
}

const fn make_keys() -> Keys {
    let mut s: u64 = 0x5106_1d0c_a7e5_b33f;
    let mut k = Keys { board: [[0; 81]; 32], hand: [[[0; 19]; 8]; 2], gote: 0 };
    macro_rules! next {
        () => {{
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        }};
    }
    let mut p = 0;
    while p < 32 {
        let mut q = 0;
        while q < 81 {
            k.board[p][q] = next!();
            q += 1;
        }
        p += 1;
    }
    let mut c = 0;
    while c < 2 {
        let mut kk = 0;
        while kk < 8 {
            let mut n = 0;
            while n < 19 {
                k.hand[c][kk][n] = next!();
                n += 1;
            }
            kk += 1;
        }
        c += 1;
    }
    k.gote = next!();
    k
}

static KEYS: Keys = make_keys();

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub board: [u8; 81],
    /// Pieces in hand by color and kind (PAWN..=ROOK).
    pub hand: [[u8; 8]; 2],
    pub stm: usize,
    pub kings: [u8; 2],
    pub ply: u16,
    pub hash: u64,
}

impl Position {
    pub fn startpos() -> Position {
        Position::from_sfen(START_SFEN).unwrap()
    }

    pub fn from_sfen(sfen: &str) -> Result<Position, String> {
        let mut parts = sfen.split_whitespace();
        let mut p = Position { board: [EMPTY; 81], hand: [[0; 8]; 2], stm: SENTE, kings: [81; 2], ply: 1, hash: 0 };
        let (mut r, mut c) = (0usize, 0usize);
        let mut promote = false;
        for ch in parts.next().ok_or("empty SFEN")?.bytes() {
            match ch {
                b'/' => {
                    r += 1;
                    c = 0;
                }
                b'1'..=b'9' => c += (ch - b'0') as usize,
                b'+' => promote = true,
                _ => {
                    let k = SFEN_LETTERS.iter().position(|&l| l == ch.to_ascii_uppercase()).ok_or(format!("bad piece {}", ch as char))? as u8;
                    if k == 0 || r > 8 || c > 8 {
                        return Err("bad board".into());
                    }
                    let k = if promote { promoted(k) } else { k };
                    promote = false;
                    let side = if ch.is_ascii_uppercase() { SENTE } else { GOTE };
                    p.put(r * 9 + c, make(side, k));
                    c += 1;
                }
            }
        }
        if p.kings[SENTE] == 81 || p.kings[GOTE] == 81 {
            return Err("each side needs a king".into());
        }
        p.stm = match parts.next().unwrap_or("b") {
            "b" => SENTE,
            "w" => GOTE,
            s => return Err(format!("bad side to move {s}")),
        };
        let hand = parts.next().unwrap_or("-");
        if hand != "-" {
            let mut count = 0u8;
            for ch in hand.bytes() {
                if ch.is_ascii_digit() {
                    count = count * 10 + (ch - b'0');
                    continue;
                }
                let k = HAND_LETTERS.iter().position(|&l| l == ch.to_ascii_uppercase()).ok_or("bad hand")? as u8;
                let side = if ch.is_ascii_uppercase() { SENTE } else { GOTE };
                for _ in 0..count.max(1) {
                    p.add_hand(side, k);
                }
                count = 0;
            }
        }
        p.ply = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
        if p.stm == GOTE {
            p.hash ^= KEYS.gote;
        }
        Ok(p)
    }

    pub fn sfen(&self) -> String {
        let mut s = String::new();
        for r in 0..9 {
            let mut empty = 0;
            for c in 0..9 {
                let p = self.board[r * 9 + c];
                if p == EMPTY {
                    empty += 1;
                    continue;
                }
                if empty > 0 {
                    s.push((b'0' + empty) as char);
                    empty = 0;
                }
                let k = kind(p);
                if k > KING {
                    s.push('+');
                }
                let l = SFEN_LETTERS[unpromoted(k) as usize] as char;
                s.push(if color(p) == SENTE { l } else { l.to_ascii_lowercase() });
            }
            if empty > 0 {
                s.push((b'0' + empty) as char);
            }
            if r < 8 {
                s.push('/');
            }
        }
        s.push_str(if self.stm == SENTE { " b " } else { " w " });
        let mut hand = String::new();
        for side in [SENTE, GOTE] {
            for k in [ROOK, BISHOP, GOLD, SILVER, KNIGHT, LANCE, PAWN] {
                let n = self.hand[side][k as usize];
                if n > 0 {
                    if n > 1 {
                        hand.push_str(&n.to_string());
                    }
                    let l = HAND_LETTERS[k as usize] as char;
                    hand.push(if side == SENTE { l } else { l.to_ascii_lowercase() });
                }
            }
        }
        if hand.is_empty() {
            hand.push('-');
        }
        format!("{s}{hand} {}", self.ply)
    }

    #[inline(always)]
    fn put(&mut self, sq: usize, p: u8) {
        self.board[sq] = p;
        self.hash ^= KEYS.board[p as usize][sq];
        if kind(p) == KING {
            self.kings[color(p)] = sq as u8;
        }
    }

    #[inline(always)]
    fn take(&mut self, sq: usize) -> u8 {
        let p = self.board[sq];
        self.board[sq] = EMPTY;
        self.hash ^= KEYS.board[p as usize][sq];
        p
    }

    #[inline(always)]
    fn add_hand(&mut self, c: usize, k: u8) {
        let n = &mut self.hand[c][k as usize];
        self.hash ^= KEYS.hand[c][k as usize][*n as usize];
        *n += 1;
        self.hash ^= KEYS.hand[c][k as usize][*n as usize];
    }

    #[inline(always)]
    fn remove_hand(&mut self, c: usize, k: u8) {
        let n = &mut self.hand[c][k as usize];
        self.hash ^= KEYS.hand[c][k as usize][*n as usize];
        *n -= 1;
        self.hash ^= KEYS.hand[c][k as usize][*n as usize];
    }

    /// Plays a pseudo-legal move. Legality is checked with `is_legal`.
    pub fn make(&self, m: Move) -> Position {
        let mut p = *self;
        let us = self.stm;
        if m.is_drop() {
            p.remove_hand(us, m.drop_kind());
            p.put(m.to(), make(us, m.drop_kind()));
        } else {
            let moving = p.take(m.from());
            if p.board[m.to()] != EMPTY {
                let captured = p.take(m.to());
                p.add_hand(us, unpromoted(kind(captured)));
            }
            let k = if m.is_promotion() { promoted(kind(moving)) } else { kind(moving) };
            p.put(m.to(), make(us, k));
        }
        p.stm ^= 1;
        p.hash ^= KEYS.gote;
        p.ply += 1;
        p
    }

    pub fn make_null(&self) -> Position {
        let mut p = *self;
        p.stm ^= 1;
        p.hash ^= KEYS.gote;
        p
    }

    /// Whether `sq` is attacked by any piece of side `by`.
    pub fn attacked(&self, sq: usize, by: usize) -> bool {
        // A piece of `by` attacks sq if sq is one of its steps or along one of its slides.
        // Look outward from sq in every direction, as seen from `by`'s pieces.
        let them = by;
        for &d in KING_STEPS {
            // The neighbour at sq - d (as `by` moves) would reach sq by stepping d.
            let back = (-d.0, -d.1);
            if let Some(from) = step(sq, them, back) {
                let p = self.board[from];
                if p != EMPTY && color(p) == them && movement(kind(p)).0.contains(&d) {
                    return true;
                }
            }
        }
        for d in [(-2, 1), (-2, -1)] {
            if let Some(from) = step(sq, them, (-d.0, -d.1)) {
                if self.board[from] == make(them, KNIGHT) {
                    return true;
                }
            }
        }
        for &d in KING_STEPS {
            let back = (-d.0, -d.1);
            let mut cur = sq;
            while let Some(next) = step(cur, them, back) {
                let p = self.board[next];
                if p != EMPTY {
                    if color(p) == them && movement(kind(p)).1.contains(&d) {
                        return true;
                    }
                    break;
                }
                cur = next;
            }
        }
        false
    }

    #[inline]
    pub fn in_check(&self) -> bool {
        self.attacked(self.kings[self.stm] as usize, self.stm ^ 1)
    }

    /// Pseudo-legal moves: everything but leaving the king attacked and the pawn-drop mate.
    pub fn generate(&self, out: &mut Vec<Move>, captures_only: bool) {
        let us = self.stm;
        for from in 0..81 {
            let p = self.board[from];
            if p == EMPTY || color(p) != us {
                continue;
            }
            let k = kind(p);
            let (steps, slides) = movement(k);
            let emit = |to: usize, out: &mut Vec<Move>| {
                let target = self.board[to];
                if captures_only && target == EMPTY {
                    // Promotions that capture nothing still matter in quiescence only rarely; skip.
                    return;
                }
                let promotable = can_promote(k) && (in_zone(us, from) || in_zone(us, to));
                let far = rank_from(us, to);
                // A pawn or lance on the last rank, or a knight on the last two, could never move again.
                let must = match k {
                    PAWN | LANCE => far == 8,
                    KNIGHT => far >= 7,
                    _ => false,
                };
                if promotable {
                    out.push(Move::normal(from, to, true));
                }
                if !must {
                    out.push(Move::normal(from, to, false));
                }
            };
            for &d in steps {
                if let Some(to) = step(from, us, d) {
                    let t = self.board[to];
                    if t == EMPTY || color(t) != us {
                        emit(to, out);
                    }
                }
            }
            for &d in slides {
                let mut cur = from;
                while let Some(to) = step(cur, us, d) {
                    let t = self.board[to];
                    if t != EMPTY && color(t) == us {
                        break;
                    }
                    emit(to, out);
                    if t != EMPTY {
                        break;
                    }
                    cur = to;
                }
            }
        }
        if captures_only {
            return;
        }
        // Drops.
        let mut pawn_files = [false; 9];
        for sq in 0..81 {
            if self.board[sq] == make(us, PAWN) {
                pawn_files[col(sq)] = true;
            }
        }
        for k in PAWN..=ROOK {
            if self.hand[us][k as usize] == 0 {
                continue;
            }
            for to in 0..81 {
                if self.board[to] != EMPTY {
                    continue;
                }
                let far = rank_from(us, to);
                let ok = match k {
                    PAWN => far < 8 && !pawn_files[col(to)],
                    LANCE => far < 8,
                    KNIGHT => far < 7,
                    _ => true,
                };
                if ok {
                    out.push(Move::drop(k, to));
                }
            }
        }
    }

    /// Whether a pseudo-legal move is legal: the mover's king is safe afterwards,
    /// and a pawn drop does not deliver mate (uchifuzume).
    pub fn is_legal(&self, m: Move, after: &Position) -> bool {
        if after.attacked(after.kings[self.stm] as usize, after.stm) {
            return false;
        }
        if m.is_drop() && m.drop_kind() == PAWN && after.in_check() {
            return after.has_legal_move_ignoring_pawn_mate();
        }
        true
    }

    fn has_legal_move_ignoring_pawn_mate(&self) -> bool {
        let mut moves = Vec::with_capacity(128);
        self.generate(&mut moves, false);
        moves.iter().any(|&m| {
            let next = self.make(m);
            !next.attacked(next.kings[self.stm] as usize, next.stm)
        })
    }

    pub fn legal_moves(&self) -> Vec<Move> {
        let mut moves = Vec::with_capacity(160);
        self.generate(&mut moves, false);
        moves.retain(|&m| self.is_legal(m, &self.make(m)));
        moves
    }

    pub fn parse_usi(&self, s: &str) -> Option<Move> {
        self.legal_moves().into_iter().find(|m| m.usi() == s)
    }

    /// Western notation as on lishogi: P-7f, Bx3c+, S*5e, Sx4b=, with the origin when ambiguous.
    pub fn notation(&self, m: Move) -> String {
        if m.is_drop() {
            return format!("{}*{}", HAND_LETTERS[m.drop_kind() as usize] as char, square_name(m.to()));
        }
        let p = self.board[m.from()];
        let k = kind(p);
        let letter = match k {
            TOKIN => "+P".to_string(),
            PRO_LANCE => "+L".into(),
            PRO_KNIGHT => "+N".into(),
            PRO_SILVER => "+S".into(),
            HORSE => "+B".into(),
            DRAGON => "+R".into(),
            KING => "K".into(),
            _ => (SFEN_LETTERS[k as usize] as char).to_string(),
        };
        let ambiguous = self
            .legal_moves()
            .iter()
            .any(|o| !o.is_drop() && o.to() == m.to() && o.from() != m.from() && self.board[o.from()] == p);
        let origin = if ambiguous { square_name(m.from()) } else { String::new() };
        let sep = if self.board[m.to()] != EMPTY { "x" } else { "-" };
        let promo = if m.is_promotion() {
            "+"
        } else if can_promote(k) && (in_zone(self.stm, m.from()) || in_zone(self.stm, m.to())) {
            "="
        } else {
            ""
        };
        format!("{letter}{origin}{sep}{}{promo}", square_name(m.to()))
    }
}

/// Counts leaf nodes of the legal move tree.
pub fn perft(pos: &Position, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let mut moves = Vec::with_capacity(160);
    pos.generate(&mut moves, false);
    let mut n = 0;
    for m in moves {
        let next = pos.make(m);
        if !pos.is_legal(m, &next) {
            continue;
        }
        n += if depth == 1 { 1 } else { perft(&next, depth - 1) };
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sfen_round_trip() {
        for s in [START_SFEN, "8l/1l+R2P3/p2pBG1pp/kps1p4/Nn1P2G2/P1P1P2PP/1PS6/1KSG3+r1/LN2+p3L w Sbgn3p 124"] {
            assert_eq!(Position::from_sfen(s).unwrap().sfen(), s);
        }
    }

    #[test]
    fn perft_start() {
        let p = Position::startpos();
        for (d, n) in [(1, 30), (2, 900), (3, 25_470), (4, 719_731), (5, 19_861_490)] {
            assert_eq!(perft(&p, d), n, "depth {d}");
        }
    }

    #[test]
    fn perft_midgame() {
        // A well-known test position with drops and promotions (from the YaneuraOu suite).
        let p = Position::from_sfen("l6nl/5+P1gk/2np1S3/p1p4Pp/3P2Sp1/1PPb2P1P/P5GS1/R8/LN4bKL w GR5pnsg 1").unwrap();
        assert_eq!(perft(&p, 1), 207);
        assert_eq!(perft(&p, 2), 28_684);
        assert_eq!(perft(&p, 3), 4_809_015);
    }

    #[test]
    fn hash_is_incremental() {
        let mut p = Position::startpos();
        let mut seed = 11u64;
        for _ in 0..3000 {
            let moves = p.legal_moves();
            if moves.is_empty() {
                p = Position::startpos();
                continue;
            }
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            p = p.make(moves[(seed % moves.len() as u64) as usize]);
            assert_eq!(p.hash, Position::from_sfen(&p.sfen()).unwrap().hash, "{}", p.sfen());
        }
    }

    #[test]
    fn pawn_drop_mate_is_illegal() {
        // Gote's king on 1a: its own lance blocks 2a and the gold on 2c covers 2b and 1b,
        // so P*1b would be mate, and is illegal. Dropping elsewhere is fine.
        let p = Position::from_sfen("7lk/9/7G1/9/9/9/9/9/K8 b P 1").unwrap();
        assert!(p.parse_usi("P*1b").is_none());
        assert!(p.parse_usi("P*1c").is_some());
    }

    #[test]
    fn notation() {
        let p = Position::startpos();
        assert_eq!(p.notation(p.parse_usi("7g7f").unwrap()), "P-7f");
        let p = p.make(p.parse_usi("7g7f").unwrap());
        let p = p.make(p.parse_usi("3c3d").unwrap());
        assert_eq!(p.notation(p.parse_usi("8h2b+").unwrap()), "Bx2b+");
        assert_eq!(p.notation(p.parse_usi("8h2b").unwrap()), "Bx2b=");
    }
}
