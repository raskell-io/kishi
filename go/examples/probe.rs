//! Average result of plain random games after each of a few candidate moves, no tree.
use kishi_engine::types::Rng;
use kishi_go::board::*;

fn main() {
    let mut b = Board::new();
    for (bm, wm) in [("A4", "A3"), ("B4", "B3"), ("C4", "C3"), ("D4", "D3"), ("E4", "E3"), ("A2", "Q16"), ("B2", "Q4"), ("C2", "D16"), ("D2", "R10"), ("E2", "K10")] {
        b.play(parse(bm).unwrap());
        b.play(parse(wm).unwrap());
    }
    let mut rng = Rng(77);
    {
        let (mut wins, mut margin, mut length) = (0, 0.0, 0);
        for _ in 0..20000 {
            let mut g = Board::new();
            while g.passes < 2 && g.moves < 1083 {
                let m = g.playout_move(&mut rng);
                g.play(m);
            }
            let s = g.playout_score();
            margin += s;
            length += g.moves as u64;
            if s > 0.0 {
                wins += 1;
            }
        }
        println!("empty board: black wins {:.3}, mean margin {:+.1}, mean length {}", wins as f64 / 20000.0, margin / 20000.0, length / 20000);
    }
    for cand in ["F3", "E18", "R11", "pass"] {
        let mut after = b.clone();
        after.play(parse(cand).unwrap());
        let (mut wins, mut margin) = (0, 0.0);
        let games = 20000;
        for _ in 0..games {
            let mut g = after.clone();
            g.passes = 0;
            let mut n = 0;
            while g.passes < 2 && n < 1083 {
                let m = g.playout_move(&mut rng);
                g.play(m);
                n += 1;
            }
            let s = g.playout_score();
            margin += s;
            if s > 0.0 {
                wins += 1;
            }
        }
        println!("{cand:>5}: black wins {:.3}, mean margin {:+.1}", wins as f64 / games as f64, margin / games as f32);
    }
}
