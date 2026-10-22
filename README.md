<p align="center"><img src="web/icon.svg" alt="" width="72"></p>

<h1 align="center">Kishi <sub>騎士</sub></h1>

<p align="center"><b>Chess against an engine that taught itself to evaluate positions, running entirely on your phone.</b></p>

Kishi is a chess engine written in Rust and compiled to WebAssembly. The web
page renders the board, and the engine thinks in a Web Worker on your device:
no server, no account, and it works offline once installed. On an iPhone,
open the page in Safari and choose **Share → Add to Home Screen**.

## How it plays

- **Board and rules**: bitboards with magic move generation, checked by perft
  against the standard suite.
- **Search**: iterative-deepening principal variation search with aspiration
  windows, a transposition table, null-move pruning, reverse futility pruning,
  razoring, late move reductions and pruning, singular extensions, static
  exchange evaluation, and killer, counter-move, history and continuation-history
  move ordering.
- **Evaluation**: an NNUE network, (768 → 512) × 2 perspectives → 1 with
  squared clipped ReLU and eight output buckets by piece count. The first layer
  is an i16 accumulator updated incrementally as moves are made; the output
  layer runs in f32 with SIMD.

## How it learned: self-play

The network is trained by a reinforcement loop, all in Rust (`tools/`):

1. **Play.** The current engine plays itself from random openings
   (`datagen`), about 5,000 nodes a move. Every quiet position is recorded with
   the search's score and, once the game ends, its result.
2. **Learn.** A new network is trained (`train`) to predict the outcome: its
   target blends the search score, as a win probability, with the game's actual
   result. The result is the reinforcement signal; it lets each generation
   correct the evaluation that played the games.
3. **Test.** The new network plays the previous one (`arena`), and the loop
   repeats with the stronger player.

Generation 0 is a hand-written evaluation (PeSTO's piece-square tables), which
plays the first games. The app ships it until a network beats it at equal
time: generation 1 wins by 85 Elo at equal nodes but is about even at equal
time, being 2.5 times slower per node. `scripts/generation.sh N` runs one
generation; `nets/LOG.md` records how each one fared.

## Levels

Levels 1 to 6 score every legal move with a search of growing depth, then pick
one at random, favouring the better moves (a softmax whose temperature falls
from 300 to 12 centipawns). Level 1 blunders freely; level 6 rarely does. Level 7
thinks for 0.8 s a move and level 8 for 2.5 s, at full strength.

## Build

Live at **https://kishi.raskell.io** (GitHub Pages, built by
`.github/workflows/pages.yml`). Pages cannot send the cross-origin isolation
headers the threaded engine needs, so the service worker adds them and the first
visit reloads once.

```sh
make serve          # build web/kishi.wasm (+ kishi-mt.wasm with nightly) and serve on :8647
make test           # perft, notation, search and SEE tests
make bench          # native search speed
```

The tools, run natively with `cargo run --release -p kishi-tools --bin <name>`:

| Tool | Does |
|---|---|
| `kishi` | UCI engine, for chess GUIs (`--net file` or `--hce`) |
| `datagen` | self-play games to training records |
| `train` | trains a network on records, writes `.nnue` |
| `arena` | two evaluations play paired games, prints the Elo difference |
| `vs` | Kishi against a UCI engine, such as Stockfish set up like lichess's levels |

The network compiled into the web build is `nets/kishi.nnue`; with an empty file
the engine falls back to the hand-written evaluation.

## Credits

Piece set "cburnett" by Colin M.L. Burnett (GPLv2+), as used by lichess.
Hand-written evaluation tables from PeSTO by Ronald Friederich.

Code: MIT.
