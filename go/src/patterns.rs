//! The 3x3 shape patterns of MoGo (Gelly, Wang, Munos and Teytaud, 2006), as
//! Pachi keeps them: hane, cuts and edge moves that a quick random game should
//! answer the last move with. Matching one is a single table lookup.
//!
//! In a pattern the centre is the candidate move. X and O are the two colours
//! (either may be the side to move: the table holds both), '.' is empty,
//! 'x' anything but X, 'o' anything but O, '#' the edge, '?' anything.

use std::sync::OnceLock;

const PATTERNS: &[&str] = &[
    "XOX...???", // hane: enclosing hane
    "XO....?.?", // hane: non-cutting hane
    "XO?X..x.?", // hane: magari
    ".O.X.....", // katatsuke or diagonal attachment
    "XO?O.o?o?", // cut: unprotected cut
    "XO?O.X???", // cut: peeped cut
    "?X?O.Oooo", // cut: de
    "X.?O.?###", // edge: chase
    "OX?X.O###", // edge: block side cut
    "?X?x.O###", // edge: block side connection
    "?XOx.x###", // edge: sagari
    "?OXX.O###", // edge: cut
];

/// Cell states in a code: two bits each.
pub const C_EMPTY: u16 = 0;
pub const C_BLACK: u16 = 1;
pub const C_WHITE: u16 = 2;
pub const C_EDGE: u16 = 3;

/// The 8 neighbours of the centre, in reading order: NW N NE W E SW S SE.
const RING: [usize; 8] = [0, 1, 2, 3, 5, 6, 7, 8];

fn matches(spec: u8, cell: u16, x: u16, o: u16) -> bool {
    match spec {
        b'X' => cell == x,
        b'O' => cell == o,
        b'.' => cell == C_EMPTY,
        b'x' => cell != x && cell != C_EDGE,
        b'o' => cell != o && cell != C_EDGE,
        b'#' => cell == C_EDGE,
        _ => true,
    }
}

/// The eight rotations and reflections of a 3x3 grid, as index maps.
fn symmetries() -> Vec<[usize; 9]> {
    let rot = |g: [usize; 9]| -> [usize; 9] { [g[6], g[3], g[0], g[7], g[4], g[1], g[8], g[5], g[2]] };
    let flip = |g: [usize; 9]| -> [usize; 9] { [g[2], g[1], g[0], g[5], g[4], g[3], g[8], g[7], g[6]] };
    let mut out = vec![];
    let mut g = [0, 1, 2, 3, 4, 5, 6, 7, 8];
    for _ in 0..4 {
        out.push(g);
        out.push(flip(g));
        g = rot(g);
    }
    out
}

fn table() -> &'static [bool] {
    static TABLE: OnceLock<Vec<bool>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = vec![false; 1 << 16];
        let syms = symmetries();
        for code in 0..(1u32 << 16) {
            let cells: Vec<u16> = (0..8).map(|i| ((code >> (2 * i)) & 3) as u16).collect();
            let mut grid = [C_EMPTY; 9];
            for (i, &r) in RING.iter().enumerate() {
                grid[r] = cells[i];
            }
            'pattern: for p in PATTERNS {
                let spec = p.as_bytes();
                for sym in &syms {
                    for (x, o) in [(C_BLACK, C_WHITE), (C_WHITE, C_BLACK)] {
                        if RING.iter().all(|&r| matches(spec[sym[r]], grid[r], x, o)) {
                            t[code as usize] = true;
                            break 'pattern;
                        }
                    }
                }
            }
        }
        t
    })
}

/// Builds the tables now rather than on first use.
pub fn init() {
    table();
}

/// Whether the 3x3 neighbourhood with this code (see `C_*`, ring order NW..SE) is a good shape move.
#[inline(always)]
pub fn is_good(code: u16) -> bool {
    table()[code as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(cells: [u16; 8]) -> u16 {
        cells.iter().enumerate().fold(0, |c, (i, &v)| c | v << (2 * i))
    }

    #[test]
    fn recognises_a_hane_and_rejects_empty_space() {
        // Black above-left, white above, black above-right, all else empty: an enclosing hane.
        assert!(is_good(code([C_BLACK, C_WHITE, C_BLACK, 0, 0, 0, 0, 0])));
        // The same with colours swapped, and turned upside down.
        assert!(is_good(code([0, 0, 0, 0, 0, C_WHITE, C_BLACK, C_WHITE])));
        assert!(!is_good(code([0; 8])));
    }

    #[test]
    fn edge_patterns_need_the_edge() {
        // Chase along the bottom edge.
        assert!(is_good(code([C_BLACK, 0, 0, C_WHITE, 0, C_EDGE, C_EDGE, C_EDGE])));
        // An empty stretch of edge is no pattern.
        assert!(!is_good(code([0, 0, 0, 0, 0, C_EDGE, C_EDGE, C_EDGE])));
    }
}
