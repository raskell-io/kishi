//! Shared pieces of the Kishi tools: the self-play game loop, random openings,
//! the training record format and Elo arithmetic.

use kishi_engine::nnue::Network;
use kishi_engine::search::{MATE_BOUND, Searcher};
use kishi_engine::types::*;
use kishi_engine::*;

/// One training position: 32 bytes.
///
/// occupancy (u64), then a nibble per occupied square in ascending order
/// (piece code 0..11), score in centipawns from white's view (i16), result
/// from white's view (0 loss, 1 draw, 2 win), side to move, four spare bytes.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Record {
    pub occ: u64,
    pub pieces: [u8; 16],
    pub score: i16,
    pub result: u8,
    pub stm: u8,
    pub spare: [u8; 4],
}

pub const RECORD_SIZE: usize = 32;

impl Record {
    pub fn new(pos: &Position, score_white: i16) -> Record {
        let mut r = Record { occ: pos.occupied(), score: score_white, stm: pos.stm as u8, ..Default::default() };
        for (i, sq) in Squares(r.occ).enumerate() {
            r.pieces[i / 2] |= pos.board[sq] << (4 * (i % 2));
        }
        r
    }

    /// (square, piece) for each piece on the board.
    pub fn pieces(&self) -> impl Iterator<Item = (usize, u8)> + '_ {
        Squares(self.occ).enumerate().map(|(i, sq)| (sq, self.pieces[i / 2] >> (4 * (i % 2)) & 15))
    }

    /// The position, without castling or en passant rights (they are not stored).
    pub fn position(&self) -> Position {
        let mut board = [NO_PIECE; 64];
        for (sq, p) in self.pieces() {
            board[sq] = p;
        }
        let mut fen = String::new();
        for rank in (0..8).rev() {
            let mut empty = 0;
            for file in 0..8 {
                let p = board[rank * 8 + file];
                if p == NO_PIECE {
                    empty += 1;
                    continue;
                }
                if empty > 0 {
                    fen.push_str(&empty.to_string());
                    empty = 0;
                }
                let c = b"pnbrqk"[kind_of(p)] as char;
                fen.push(if color_of(p) == WHITE { c.to_ascii_uppercase() } else { c });
            }
            if empty > 0 {
                fen.push_str(&empty.to_string());
            }
            if rank > 0 {
                fen.push('/');
            }
        }
        fen.push_str(if self.stm == 0 { " w - - 0 1" } else { " b - - 0 1" });
        Position::from_fen(&fen).unwrap()
    }

    pub fn to_bytes(&self) -> [u8; RECORD_SIZE] {
        let mut b = [0u8; RECORD_SIZE];
        b[..8].copy_from_slice(&self.occ.to_le_bytes());
        b[8..24].copy_from_slice(&self.pieces);
        b[24..26].copy_from_slice(&self.score.to_le_bytes());
        b[26] = self.result;
        b[27] = self.stm;
        b
    }

    pub fn from_bytes(b: &[u8]) -> Record {
        Record {
            occ: u64::from_le_bytes(b[..8].try_into().unwrap()),
            pieces: b[8..24].try_into().unwrap(),
            score: i16::from_le_bytes([b[24], b[25]]),
            result: b[26],
            stm: b[27],
            spare: [0; 4],
        }
    }
}

/// Which evaluation a player uses.
#[derive(Clone)]
pub enum Brain {
    Handwritten,
    Net(&'static Network),
}

impl Brain {
    pub fn load(spec: &str) -> Brain {
        if spec == "hce" {
            return Brain::Handwritten;
        }
        let bytes = std::fs::read(spec).unwrap_or_else(|e| panic!("read {spec}: {e}"));
        Brain::Net(Box::leak(Box::new(Network::from_bytes(&bytes).unwrap_or_else(|e| panic!("{spec}: {e}")))))
    }

    pub fn searcher(&self, tt_mb: usize) -> Searcher {
        let mut s = Searcher::new(tt_mb);
        s.set_network(match self {
            Brain::Handwritten => None,
            Brain::Net(n) => Some(n),
        });
        s
    }
}

/// A random but playable opening: `plies` random legal moves, kept only if a
/// quick search judges the result roughly balanced.
pub fn random_opening(rng: &mut Rng, plies: usize, judge: &mut Searcher) -> (Position, Vec<u64>) {
    loop {
        let mut pos = Position::startpos();
        let mut history = vec![];
        let mut ok = true;
        for _ in 0..plies {
            let moves = pos.legal_moves();
            if moves.is_empty() {
                ok = false;
                break;
            }
            history.push(pos.hash);
            pos = pos.make(moves[rng.below(moves.len())]);
        }
        if !ok || pos.legal_moves().is_empty() {
            continue;
        }
        let r = judge.search(&pos, &history, Limits { depth: 10, nodes: 20_000, soft_nodes: 4_000, ..Default::default() }, &mut |_| {});
        if r.score.abs() <= 250 {
            return (pos, history);
        }
    }
}

pub enum Outcome {
    WhiteWins,
    Draw,
    BlackWins,
}

impl Outcome {
    /// From white's view: 0 loss, 1 draw, 2 win.
    pub fn code(&self) -> u8 {
        match self {
            Outcome::BlackWins => 0,
            Outcome::Draw => 1,
            Outcome::WhiteWins => 2,
        }
    }
}

pub struct GameRecord {
    pub outcome: Outcome,
    /// Quiet positions with their search scores, white's view.
    pub positions: Vec<Record>,
}

/// Plays one game between two searchers from `pos`. `limits` applies to every move.
pub fn play_game(
    white: &mut Searcher,
    black: &mut Searcher,
    start: Position,
    mut history: Vec<u64>,
    limits: Limits,
) -> GameRecord {
    let mut pos = start;
    let mut positions = vec![];
    let (mut win_streak, mut draw_streak) = (0, 0);
    let mut plies = 0;
    white.clear();
    black.clear();
    let outcome = loop {
        if pos.legal_moves().is_empty() {
            break if !pos.in_check() {
                Outcome::Draw
            } else if pos.stm == WHITE {
                Outcome::BlackWins
            } else {
                Outcome::WhiteWins
            };
        }
        if pos.halfmove >= 100 || pos.insufficient_material() || history.iter().filter(|&&h| h == pos.hash).count() >= 2 {
            break Outcome::Draw;
        }
        let searcher = if pos.stm == WHITE { &mut *white } else { &mut *black };
        let r = searcher.search(&pos, &history, limits, &mut |_| {});
        let white_score = if pos.stm == WHITE { r.score } else { -r.score };

        if !pos.in_check() && r.best.is_quiet() && r.score.abs() < MATE_BOUND {
            positions.push(Record::new(&pos, white_score.clamp(-i16::MAX as i32, i16::MAX as i32) as i16));
        }

        // Adjudicate once both sides agree for long enough.
        if white_score.abs() >= 2000 {
            win_streak += 1;
            if win_streak >= 6 {
                break if white_score > 0 { Outcome::WhiteWins } else { Outcome::BlackWins };
            }
        } else {
            win_streak = 0;
        }
        if plies >= 80 && white_score.abs() <= 8 {
            draw_streak += 1;
            if draw_streak >= 12 {
                break Outcome::Draw;
            }
        } else {
            draw_streak = 0;
        }

        history.push(pos.hash);
        pos = pos.make(r.best);
        plies += 1;
    };
    let code = outcome.code();
    for p in &mut positions {
        p.result = code;
    }
    GameRecord { outcome, positions }
}

/// Elo difference and its 95% interval from wins, draws and losses.
pub fn elo(w: u32, d: u32, l: u32) -> (f64, f64) {
    let n = (w + d + l) as f64;
    if n == 0.0 {
        return (0.0, 0.0);
    }
    let score = (w as f64 + d as f64 / 2.0) / n;
    let to_elo = |s: f64| -400.0 * (1.0 / s.clamp(1e-4, 1.0 - 1e-4) - 1.0).log10();
    let var = (w as f64 * (1.0 - score).powi(2) + d as f64 * (0.5 - score).powi(2) + l as f64 * score.powi(2)) / n;
    let margin = 1.96 * (var / n).sqrt();
    (to_elo(score), (to_elo(score + margin) - to_elo(score - margin)) / 2.0)
}

/// Reads `--key value` from the command line.
pub fn arg<T: std::str::FromStr>(args: &[String], key: &str, default: T) -> T {
    args.iter().position(|a| a == key).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(default)
}
