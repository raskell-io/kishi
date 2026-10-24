# The tools

`kishi-tools` holds the native programs around the engines: a UCI front end,
the self-play training loop, and matches against each other and against
established engines. None of it ships to the browser. Build them all with
`make tools` (or `cargo build --release -p kishi-tools`); they land in
`target/release/`.

| Binary | Does |
|---|---|
| `kishi` | the chess engine over UCI, for chess GUIs and match runners |
| `datagen` | self-play games, written as training records |
| `train` | trains a network on records, writes `.nnue` |
| `arena` | two chess evaluations play paired games; prints the Elo difference |
| `vs` | Kishi's chess against a UCI engine (Stockfish set up like lichess's levels, or an older Kishi) |
| `shogi-vs` | Kishi's shogi against a USI engine (Fairy-Stockfish by default) |
| `go-vs` | Kishi's Go against GNU Go over GTP |

Shared code (the game loop, random openings, the record format, Elo arithmetic,
argument parsing) is in `src/lib.rs`. Every option is `--name value`.

## `kishi`: UCI

```sh
target/release/kishi                    # the embedded network, or the hand-written eval
target/release/kishi --net nets/gen1.nnue
target/release/kishi --hce
target/release/kishi bench              # 12 positions at depth 12: nodes and speed
```

Options: `Hash` (MB) and `Threads` (Lazy SMP). Beyond UCI: `bench [depth]`,
`perft <depth>`, `eval` (the static evaluation of the current position), `d`
(its FEN).

## The self-play loop

The network learns from games the engine plays against itself, in
generations. Generation 0 is the hand-written evaluation.

```text
datagen (generation n's player) ──► data/gen<n>.bin ──► train ──► nets/gen<n+1>.nnue ──► arena vs generation n
```

`scripts/generation.sh N [positions] [epochs]` runs one round: it trains
`nets/genN.nnue` on the last three generations' data, plays it against the
previous generation (400 games at 20,000 nodes), logs the result in
`nets/LOG.md`, and generates `data/genN.bin` with the new network.

### `datagen`

```sh
target/release/datagen --brain hce --out data/gen0.bin --positions 10000000 [--nodes 5000] [--threads 10] [--seed 1]
```

Each thread plays games against itself:

- **Openings.** Eight or nine random legal moves, kept only if a quick search
  (4,000 nodes) judges the result within 250 centipawns.
- **Moves.** A search to about `--nodes` (it finishes the iteration it is in,
  at most eight times that).
- **Records.** Every position not in check whose best move is quiet and whose
  score is not a mate, with the search score.
- **Adjudication.** A win once the score has been 2,000 or more for six plies
  in a row; a draw after move 80 once it has been within 8 for twelve plies;
  otherwise mate, stalemate, repetition, the fifty-move rule or bare material.

When the game ends, its result is written into all of its records. About
2,000 records a second on ten cores with the hand-written evaluation. The file
is appended to, so runs can be stopped and resumed.

### Records

32 bytes each (`Record` in `src/lib.rs`):

| Bytes | Content |
|---|---|
| 8 | occupancy bitboard |
| 16 | a nibble per occupied square, in ascending square order: the piece (0–11) |
| 2 | search score, centipawns, white's view, i16 |
| 1 | result, white's view: 0 loss, 1 draw, 2 win |
| 1 | side to move |
| 4 | spare |

Castling and en passant rights are not stored; the network does not use them.

### `train`

```sh
target/release/train --data data/gen0.bin[,data/gen1.bin…] --out nets/gen1.nnue \
  [--epochs 30] [--lr 0.001] [--wdl 0.3] [--batch 16384] [--threads 10] [--k 0]
```

- **Target.** The network predicts the game's outcome: its target blends the
  search score as a win probability, `sigmoid(score / K)`, with the actual
  result, `wdl` of the way. The result is the reinforcement signal.
- **K.** The scale that turns a score into a probability is fitted to the data
  (the K under which scores best predict results; 165 on generation 0) unless
  `--k` sets it. With a wrong K the network's centipawns come out on another
  scale than the search's pruning margins; an early run with K = 400 lost to
  the hand-written evaluation for exactly that reason.
- **Optimiser.** Adam, a cosine learning rate from `lr` down to `lr / 50`,
  mean squared error on the predicted probability. Batches are split across
  threads, each with its own gradient buffers.
- **Data.** Every record is loaded and shuffled once; 1% (at most 200,000) is
  held out for validation. Each epoch visits the batches in a new order.

After the last epoch it writes the quantised network and reports how far the
integer network the engine runs is from the float one it trained (about 2
centipawns on average). `cargo test -p kishi-tools --bin train` checks the
gradients against finite differences.

### `arena`

```sh
target/release/arena --a nets/gen2.nnue --b nets/gen1.nnue [--games 400] [--nodes 20000 | --ms 100] [--threads 10]
```

Games come in pairs from the same random opening, colours swapped. Equal
nodes measures the evaluation; equal time (`--ms`) is what matters in play,
since a network costs speed. Give a match the cores to itself when timing it.

## Measuring against other engines

| Command | Plays |
|---|---|
| `vs --lichess 7 --ms 1000 --games 20` | Stockfish set up like lichess's level 7 (skill, depth limit, think time) |
| `vs --elo 2400` | Stockfish with its own UCI_Elo limit |
| `vs --engine bin/kishi-old --nodes 10000 --games 600` | an older Kishi at fixed nodes: an A/B test of a search change |
| `vs --lichess 7 --ms 3000 --kishi-threads 4` | the same, with Kishi on four threads, as the phone plays level 7 |
| `shogi-vs --skill 10 --ms 500 --their-ms 500` | Fairy-Stockfish at a skill level from −20 to 20 |
| `go-vs --level 1 --ms 1000 [--no-resign]` | GNU Go at a level from 1 to 10, Chinese rules, komi 7.5 |

Install the opponents with Homebrew: `stockfish`, `fairy-stockfish`, `gnu-go`.
Every match checks the opponent's moves against Kishi's move generator, so a
rules bug shows up as a failed game rather than a quiet desync.
`GO_TRACE=1 go-vs …` prints each move, Kishi's win rate, and the final board.
