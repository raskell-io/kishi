//! The Go board: 19x19 points on a padded grid, stones in groups with their
//! liberties as bitsets, captures, suicide, ko, and area scoring.
//!
//! Points are indices into a (19 + 2)^2 grid whose outer ring is a border, so
//! neighbours never need bounds checks. x runs left to right, y top to bottom.

use crate::patterns;
use kishi_engine::types::Rng;

pub const N: usize = 19;
pub const W: usize = N + 2;
pub const CELLS: usize = W * W;
const WORDS: usize = CELLS.div_ceil(64);

pub const EMPTY: u8 = 0;
pub const BLACK: u8 = 1;
pub const WHITE: u8 = 2;
pub const BORDER: u8 = 3;

/// The pass move.
pub const PASS: u16 = 0;
pub const KOMI: f32 = 7.5;

#[inline(always)]
pub const fn opponent(c: u8) -> u8 {
    3 - c
}

#[inline(always)]
pub const fn point(x: usize, y: usize) -> usize {
    (y + 1) * W + x + 1
}

#[inline(always)]
pub const fn xy(p: usize) -> (usize, usize) {
    (p % W - 1, p / W - 1)
}

#[inline(always)]
const fn neighbours(p: usize) -> [usize; 4] {
    [p - W, p - 1, p + 1, p + W]
}

#[inline(always)]
const fn diagonals(p: usize) -> [usize; 4] {
    [p - W - 1, p - W + 1, p + W - 1, p + W + 1]
}

const COLUMNS: &[u8; 19] = b"ABCDEFGHJKLMNOPQRST";

/// GTP coordinates: a column letter (no I) and a row counted from the bottom, e.g. "D4".
pub fn name(p: u16) -> String {
    if p == PASS {
        return "pass".into();
    }
    let (x, y) = xy(p as usize);
    format!("{}{}", COLUMNS[x] as char, N - y)
}

pub fn parse(s: &str) -> Option<u16> {
    if s.eq_ignore_ascii_case("pass") {
        return Some(PASS);
    }
    let col = s.as_bytes().first()?.to_ascii_uppercase();
    let x = COLUMNS.iter().position(|&c| c == col)?;
    let row: usize = s[1..].parse().ok()?;
    (1..=N).contains(&row).then(|| point(x, N - row) as u16)
}

struct Keys {
    stone: [[u64; CELLS]; 3],
    white: u64,
}

const fn make_keys() -> Keys {
    let mut s: u64 = 0x60_6f_6b_69_73_68_69_01;
    let mut k = Keys { stone: [[0; CELLS]; 3], white: 0 };
    let mut c = 0;
    while c < 3 {
        let mut p = 0;
        while p < CELLS {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            k.stone[c][p] = s;
            p += 1;
        }
        c += 1;
    }
    s ^= s << 13;
    s ^= s >> 7;
    s ^= s << 17;
    k.white = s;
    k
}

static KEYS: Keys = make_keys();

#[derive(Clone)]
pub struct Board {
    pub cells: [u8; CELLS],
    /// The representative stone of each stone's group.
    head: [u16; CELLS],
    /// The next stone of the same group, in a circular list.
    next: [u16; CELLS],
    /// Stones in a group, kept at its head.
    size: [u16; CELLS],
    /// Liberties of a group as a bitset of points, kept at its head.
    libs: [[u64; WORDS]; CELLS],
    /// Empty points, for picking random moves, and where each sits in the list.
    empties: [u16; N * N],
    empty_at: [u16; CELLS],
    n_empty: usize,
    pub to_move: u8,
    /// The point that may not be retaken this move, or 0.
    pub ko: u16,
    pub captures: [u16; 3],
    pub passes: u8,
    pub moves: u16,
    pub last: u16,
    pub hash: u64,
}

impl Default for Board {
    fn default() -> Self {
        Board::new()
    }
}

impl Board {
    pub fn new() -> Board {
        let mut b = Board {
            cells: [BORDER; CELLS],
            head: [0; CELLS],
            next: [0; CELLS],
            size: [0; CELLS],
            libs: [[0; WORDS]; CELLS],
            empties: [0; N * N],
            empty_at: [0; CELLS],
            n_empty: 0,
            to_move: BLACK,
            ko: 0,
            captures: [0; 3],
            passes: 0,
            moves: 0,
            last: PASS,
            hash: 0,
        };
        for y in 0..N {
            for x in 0..N {
                let p = point(x, y);
                b.cells[p] = EMPTY;
                b.add_empty(p);
            }
        }
        b
    }

    #[inline(always)]
    fn add_empty(&mut self, p: usize) {
        self.empty_at[p] = self.n_empty as u16;
        self.empties[self.n_empty] = p as u16;
        self.n_empty += 1;
    }

    #[inline(always)]
    fn remove_empty(&mut self, p: usize) {
        let i = self.empty_at[p] as usize;
        self.n_empty -= 1;
        let last = self.empties[self.n_empty];
        self.empties[i] = last;
        self.empty_at[last as usize] = i as u16;
    }

    #[inline(always)]
    fn lib_count(&self, h: usize) -> u32 {
        self.libs[h].iter().map(|w| w.count_ones()).sum()
    }

    /// The liberties of the group at `p`.
    pub fn liberties(&self, p: usize) -> u32 {
        self.lib_count(self.head[p] as usize)
    }

    /// The only liberty of a group in atari.
    fn last_liberty(&self, h: usize) -> usize {
        for (i, w) in self.libs[h].iter().enumerate() {
            if *w != 0 {
                return i * 64 + w.trailing_zeros() as usize;
            }
        }
        0
    }

    #[inline(always)]
    fn set_lib(&mut self, h: usize, p: usize) {
        self.libs[h][p / 64] |= 1 << (p % 64);
    }

    #[inline(always)]
    fn clear_lib(&mut self, h: usize, p: usize) {
        self.libs[h][p / 64] &= !(1 << (p % 64));
    }

    pub fn is_legal(&self, p: u16, c: u8) -> bool {
        if p == PASS {
            return true;
        }
        let p = p as usize;
        if self.cells[p] != EMPTY || p == self.ko as usize {
            return false;
        }
        for n in neighbours(p) {
            match self.cells[n] {
                EMPTY => return true,
                BORDER => {}
                s => {
                    let libs = self.lib_count(self.head[n] as usize);
                    // Joining a group with another liberty, or taking a group's last one.
                    if (s == c && libs > 1) || (s != c && libs == 1) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// A point surrounded by `c`'s stones, whose diagonals `c` mostly holds: filling it only hurts.
    pub fn is_eye(&self, p: usize, c: u8) -> bool {
        if neighbours(p).iter().any(|&n| self.cells[n] != c && self.cells[n] != BORDER) {
            return false;
        }
        let (mut enemy, mut edge) = (0, 0);
        for d in diagonals(p) {
            match self.cells[d] {
                BORDER => edge = 1,
                x if x == opponent(c) => enemy += 1,
                _ => {}
            }
        }
        enemy + edge < 2
    }

    /// Plays a legal move for the side to move (a pass included).
    pub fn play(&mut self, p: u16) {
        let c = self.to_move;
        self.to_move = opponent(c);
        self.hash ^= KEYS.white;
        self.moves += 1;
        self.last = p;
        if p == PASS {
            self.passes += 1;
            self.ko = 0;
            return;
        }
        self.passes = 0;
        let p = p as usize;
        self.remove_empty(p);
        self.cells[p] = c;
        self.hash ^= KEYS.stone[c as usize][p];
        self.head[p] = p as u16;
        self.next[p] = p as u16;
        self.size[p] = 1;
        self.libs[p] = [0; WORDS];
        for n in neighbours(p) {
            if self.cells[n] == EMPTY {
                self.set_lib(p, n);
            }
        }
        for n in neighbours(p) {
            let s = self.cells[n];
            if s == c {
                let (a, b) = (self.head[p] as usize, self.head[n] as usize);
                if a != b {
                    self.merge(a, b);
                }
            } else if s == opponent(c) {
                let h = self.head[n] as usize;
                self.clear_lib(h, p);
            }
        }
        let mine = self.head[p] as usize;
        self.clear_lib(mine, p);
        let mut captured = 0;
        let mut captured_at = 0;
        for n in neighbours(p) {
            if self.cells[n] == opponent(c) {
                let h = self.head[n] as usize;
                if self.lib_count(h) == 0 {
                    captured += self.size[h];
                    captured_at = n;
                    self.capture(h);
                }
            }
        }
        self.captures[c as usize] += captured;
        // A single stone that took a single stone and is left in atari: that point is ko.
        let mine = self.head[p] as usize;
        self.ko = if captured == 1 && self.size[mine] == 1 && self.lib_count(mine) == 1 { captured_at as u16 } else { 0 };
    }

    fn merge(&mut self, a: usize, b: usize) {
        // Keep the larger group's head; relabel the smaller one's stones.
        let (keep, gone) = if self.size[a] >= self.size[b] { (a, b) } else { (b, a) };
        let mut s = gone;
        loop {
            self.head[s] = keep as u16;
            s = self.next[s] as usize;
            if s == gone {
                break;
            }
        }
        self.next.swap(keep, gone);
        self.size[keep] += self.size[gone];
        for i in 0..WORDS {
            self.libs[keep][i] |= self.libs[gone][i];
        }
    }

    fn capture(&mut self, h: usize) {
        let mut s = h;
        loop {
            let nxt = self.next[s] as usize;
            let c = self.cells[s];
            self.cells[s] = EMPTY;
            self.hash ^= KEYS.stone[c as usize][s];
            self.add_empty(s);
            for n in neighbours(s) {
                let t = self.cells[n];
                if t == opponent(c) {
                    let g = self.head[n] as usize;
                    self.set_lib(g, s);
                }
            }
            s = nxt;
            if s == h {
                break;
            }
        }
    }

    /// Every legal move for the side to move, without filling its own eyes, plus pass.
    pub fn moves_for_search(&self) -> Vec<u16> {
        let c = self.to_move;
        let mut out: Vec<u16> = self.empties[..self.n_empty]
            .iter()
            .copied()
            .filter(|&p| self.is_legal(p, c) && !self.is_eye(p as usize, c))
            .collect();
        out.push(PASS);
        out
    }

    /// Liberties a stone of `c` at `p` would have, counting the groups it joins (captures ignored).
    pub fn libs_after(&self, p: usize, c: u8) -> u32 {
        let mut set = [0u64; WORDS];
        for n in neighbours(p) {
            match self.cells[n] {
                EMPTY => set[n / 64] |= 1 << (n % 64),
                s if s == c => {
                    let h = self.head[n] as usize;
                    for i in 0..WORDS {
                        set[i] |= self.libs[h][i];
                    }
                }
                _ => {}
            }
        }
        set[p / 64] &= !(1 << (p % 64));
        set.iter().map(|w| w.count_ones()).sum()
    }

    /// Whether a move takes an enemy group's last liberty.
    pub fn captures_with(&self, p: usize, c: u8) -> bool {
        neighbours(p).iter().any(|&n| self.cells[n] == opponent(c) && self.lib_count(self.head[n] as usize) == 1)
    }

    /// Stones of the enemy groups a move at `p` would capture.
    pub fn capture_size(&self, p: usize, c: u8) -> u16 {
        let mut heads = [0usize; 4];
        let mut total = 0;
        for (i, n) in neighbours(p).into_iter().enumerate() {
            if self.cells[n] == opponent(c) {
                let h = self.head[n] as usize;
                if self.lib_count(h) == 1 && !heads[..i].contains(&h) {
                    total += self.size[h];
                }
                heads[i] = h;
            }
        }
        total
    }

    /// Stones of our groups in atari that a move at `p` would bring to two liberties or more.
    pub fn rescue_size(&self, p: usize, c: u8) -> u16 {
        let mut total = 0;
        let mut heads = [0usize; 4];
        for (i, n) in neighbours(p).into_iter().enumerate() {
            if self.cells[n] == c {
                let h = self.head[n] as usize;
                if self.lib_count(h) == 1 && !heads[..i].contains(&h) {
                    total += self.size[h];
                }
                heads[i] = h;
            }
        }
        if total > 0 && self.libs_after(p, c) >= 2 { total } else { 0 }
    }

    /// Stones of `c` in a group of `size` or more next to `p`, which a stone at `p` would join.
    fn joined_size(&self, p: usize, c: u8) -> u16 {
        let mut heads = [0usize; 4];
        let mut total = 1;
        for (i, n) in neighbours(p).into_iter().enumerate() {
            if self.cells[n] == c {
                let h = self.head[n] as usize;
                if !heads[..i].contains(&h) {
                    total += self.size[h];
                }
                heads[i] = h;
            }
        }
        total
    }

    /// The 3x3 neighbourhood of `p` as a pattern code (see patterns.rs), ring order NW..SE.
    #[inline]
    pub fn pattern_code(&self, p: usize) -> u16 {
        let ring = [p - W - 1, p - W, p - W + 1, p - 1, p + 1, p + W - 1, p + W, p + W + 1];
        let mut code = 0u16;
        for (i, q) in ring.into_iter().enumerate() {
            let v = match self.cells[q] {
                EMPTY => patterns::C_EMPTY,
                BLACK => patterns::C_BLACK,
                WHITE => patterns::C_WHITE,
                _ => patterns::C_EDGE,
            };
            code |= v << (2 * i);
        }
        code
    }

    /// Whether a move puts a group of two or more stones into atari without capturing.
    #[inline]
    pub fn is_bad_self_atari(&self, p: usize, c: u8) -> bool {
        self.libs_after(p, c) == 1 && !self.captures_with(p, c) && self.joined_size(p, c) >= 2
    }

    /// A move for a quick random game: answer the last move's ataris first, then play a
    /// good shape next to it, then any legal move that neither fills an own eye nor
    /// throws a group into atari.
    pub fn playout_move(&self, rng: &mut Rng) -> u16 {
        let c = self.to_move;
        if self.last != PASS {
            let last = self.last as usize;
            // Capture a group the last move left in atari, or rescue one of ours it put there.
            for n in neighbours(last).into_iter().chain([last]) {
                let s = self.cells[n];
                if s == EMPTY || s == BORDER {
                    continue;
                }
                let h = self.head[n] as usize;
                if self.lib_count(h) != 1 {
                    continue;
                }
                let lib = self.last_liberty(h);
                if s == opponent(c) {
                    if self.is_legal(lib as u16, c) {
                        return lib as u16;
                    }
                } else if self.is_legal(lib as u16, c) && self.libs_after(lib, c) >= 2 {
                    return lib as u16;
                }
            }
        }
        if self.last != PASS {
            // Shape around the last move: hane, cuts, blocks (the MoGo patterns).
            let last = self.last as usize;
            let mut good = [0u16; 8];
            let mut n = 0;
            for q in [last - W - 1, last - W, last - W + 1, last - 1, last + 1, last + W - 1, last + W, last + W + 1] {
                if self.cells[q] == EMPTY
                    && patterns::is_good(self.pattern_code(q))
                    && self.is_legal(q as u16, c)
                    && !self.is_bad_self_atari(q, c)
                {
                    good[n] = q as u16;
                    n += 1;
                }
            }
            if n > 0 {
                return good[rng.below(n)];
            }
        }
        if self.n_empty == 0 {
            return PASS;
        }
        let start = rng.below(self.n_empty);
        for i in 0..self.n_empty {
            let p = self.empties[(start + i) % self.n_empty] as usize;
            if !self.is_legal(p as u16, c) || self.is_eye(p, c) {
                continue;
            }
            // Self-atari of more than a stone or two is almost always a mistake.
            if self.is_bad_self_atari(p, c) && rng.below(8) != 0 {
                continue;
            }
            return p as u16;
        }
        PASS
    }

    /// Area score of a finished random game, black minus white minus komi: every
    /// point is a stone or an empty point bordered by one colour only.
    pub fn playout_score(&self) -> f32 {
        let mut s = 0i32;
        for y in 0..N {
            for x in 0..N {
                let p = point(x, y);
                match self.cells[p] {
                    BLACK => s += 1,
                    WHITE => s -= 1,
                    _ => {
                        let (mut b, mut w) = (false, false);
                        for n in neighbours(p) {
                            b |= self.cells[n] == BLACK;
                            w |= self.cells[n] == WHITE;
                        }
                        if b && !w {
                            s += 1;
                        } else if w && !b {
                            s -= 1;
                        }
                    }
                }
            }
        }
        s as f32 - KOMI
    }

    /// Tromp-Taylor area score as the stones stand: stones plus empty regions that touch
    /// only one colour, black minus white minus komi.
    pub fn area_score(&self, dead: &[bool]) -> f32 {
        let mut owner = [0u8; CELLS];
        let mut s = 0i32;
        let colour = |p: usize| if dead[p] { EMPTY } else { self.cells[p] };
        for y in 0..N {
            for x in 0..N {
                let p = point(x, y);
                match colour(p) {
                    BLACK => s += 1,
                    WHITE => s -= 1,
                    _ if owner[p] == 0 => {
                        // Flood the empty region and see whose stones it touches.
                        let mut region = vec![p];
                        let mut stack = vec![p];
                        owner[p] = 9;
                        let (mut b, mut w) = (false, false);
                        while let Some(q) = stack.pop() {
                            for n in neighbours(q) {
                                match colour(n) {
                                    BLACK => b = true,
                                    WHITE => w = true,
                                    EMPTY if owner[n] == 0 && self.cells[n] != BORDER => {
                                        owner[n] = 9;
                                        region.push(n);
                                        stack.push(n);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        let who = if b && !w { BLACK } else if w && !b { WHITE } else { 3 };
                        for &q in &region {
                            owner[q] = who;
                        }
                        if who == BLACK {
                            s += region.len() as i32;
                        } else if who == WHITE {
                            s -= region.len() as i32;
                        }
                    }
                    _ => {}
                }
            }
        }
        s as f32 - KOMI
    }

    /// The board as 361 characters, row by row from the top: '.', 'x' (black), 'o' (white).
    pub fn picture(&self) -> String {
        let mut s = String::with_capacity(N * N);
        for y in 0..N {
            for x in 0..N {
                s.push(match self.cells[point(x, y)] {
                    BLACK => 'x',
                    WHITE => 'o',
                    _ => '.',
                });
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> u16 {
        parse(s).unwrap()
    }

    fn play_all(moves: &[&str]) -> Board {
        let mut b = Board::new();
        for m in moves {
            let p = at(m);
            assert!(b.is_legal(p, b.to_move), "{m} illegal");
            b.play(p);
        }
        b
    }

    #[test]
    fn coordinates() {
        assert_eq!(name(at("D4")), "D4");
        assert_eq!(name(at("T19")), "T19");
        assert_eq!(name(at("A1")), "A1");
        assert!(parse("I5").is_none());
    }

    #[test]
    fn captures_a_stone() {
        // White's stone on B2 loses its last liberty.
        let b = play_all(&["A2", "B2", "B1", "T19", "C2", "T18", "B3"]);
        assert_eq!(b.cells[at("B2") as usize], EMPTY);
        assert_eq!(b.captures[BLACK as usize], 1);
    }

    #[test]
    fn suicide_is_illegal() {
        let b = play_all(&["A2", "T19", "B1"]);
        // White to move: A1 has no liberties and captures nothing.
        assert!(!b.is_legal(at("A1"), WHITE));
    }

    #[test]
    fn ko_cannot_be_retaken_at_once() {
        // The classic shape: black takes at C3, white may not retake at once.
        let b = play_all(&["B3", "C3", "C2", "D2", "C4", "D4", "T19", "E3", "D3"]);
        assert_eq!(b.cells[at("C3") as usize], EMPTY, "white C3 captured");
        assert_eq!(b.ko, at("C3"));
        assert!(!b.is_legal(at("C3"), WHITE));
    }

    #[test]
    fn groups_merge_and_count_liberties() {
        let b = play_all(&["D4", "Q16", "D5", "Q4", "E4"]);
        assert_eq!(b.liberties(at("D4") as usize), 7);
    }

    #[test]
    fn empty_board_score_is_minus_komi() {
        let b = Board::new();
        assert_eq!(b.area_score(&[false; CELLS]), -KOMI);
    }

    #[test]
    fn random_games_finish_and_keep_the_board_consistent() {
        let mut rng = Rng(5);
        for _ in 0..20 {
            let mut b = Board::new();
            while b.passes < 2 && b.moves < 1000 {
                let m = b.playout_move(&mut rng);
                b.play(m);
            }
            // Every stone's group head agrees with its neighbours of the same colour.
            for y in 0..N {
                for x in 0..N {
                    let p = point(x, y);
                    if b.cells[p] == EMPTY || b.cells[p] == BORDER {
                        continue;
                    }
                    for n in neighbours(p) {
                        if b.cells[n] == b.cells[p] {
                            assert_eq!(b.head[n], b.head[p]);
                        }
                    }
                    assert!(b.liberties(p) > 0);
                }
            }
            assert!(b.moves < 1000);
        }
    }
}
