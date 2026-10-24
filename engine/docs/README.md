# The chess engine

`kishi-engine` is a classical alpha-beta chess engine with an NNUE evaluation.
It is a library (for the tools and tests) and, with the `web` feature, the
WebAssembly module the app plays. See also [the network](nnue.md).

| Module | Holds |
|---|---|
| `types.rs` | squares, pieces, the 16-bit `Move`, `MoveList`, bit helpers, the xorshift `Rng` |
| `attacks.rs` | leaper tables (compile time) and magic bitboards for sliders (start-up, fixed seed) |
| `position.rs` | the `Position`: bitboards and mailbox, FEN, making moves, Zobrist hashing, SAN |
| `movegen.rs` | pseudo-legal generation, all moves or captures only; `perft` |
| `hce.rs` | the hand-written evaluation (PeSTO's tapered piece-square tables) |
| `nnue.rs` | network inference: accumulators, incremental updates, the output layer |
| `tt.rs` | the lock-free transposition table |
| `search.rs` | iterative deepening PVS, quiescence, pruning, ordering, Lazy SMP, time |
| `web.rs` | the browser ABI, levels, the opening book |
| `web/threads.rs` | helper threads for the threaded wasm build |

## Board

Squares count from a1 = 0 to h8 = 63; a piece is `colour * 6 + kind`. A
`Position` keeps a bitboard per kind and per colour plus a 64-byte mailbox, the
side to move, castling rights, the en passant square, the move counters and the
hash. It is small and `Copy`: `make` returns a new position, so the search never
unmakes a move, and checking legality is making the move and asking whether
the mover's king is attacked.

Two details keep hashes honest for repetition: the en passant square is only
set (and hashed) when a pawn could actually capture there, and castling rights
that the pieces no longer support are dropped when a FEN is read.

Moves are 16 bits: from (6), to (6) and a 4-bit flag (quiet, double push, the
two castles, capture, en passant, and the eight promotions with and without
capture). Castling is written as the king's move.

Move generation is verified by `perft` against the standard suite (start
position to depth 5, Kiwipete, and the usual endgame and promotion positions)
in `movegen.rs`'s tests.

## Search

`Searcher::search` runs iterative deepening with aspiration windows (±20 from
depth 5, widening by half each failure) around a principal variation search:

- **Transposition table.** 16-byte slots, one per bucket, replaced by depth and
  age. Each slot stores the data and the hash XORed with it; a slot torn by two
  threads writing at once fails that check and reads as empty, so threads share
  the table without locks.
- **Pruning before the move loop** (not in PV nodes, not in check): reverse
  futility (`eval − 75 × (depth − improving) ≥ beta`, up to depth 8), razoring
  (`eval + 250 × depth < alpha`, up to depth 3), and null move
  (`R = 3 + depth/3 + min((eval − beta)/200, 3)`).
- **Internal iterative reduction** when a node of depth 4 or more has no hash
  move.
- **In the move loop**: late move pruning (`(3 + depth²) / (2 − improving)`
  quiets), futility pruning, and SEE pruning of quiets and captures.
- **Singular extensions** from depth 8: a hash move clearly better than every
  alternative is searched a ply deeper; if the alternatives also beat beta, the
  node returns at once (multi-cut).
- **Late move reductions** from `0.8 + ln(depth) × ln(moves) / 2.3`, less for PV
  nodes, checks, killers and moves with good history, more for cut nodes and
  when the position is not improving.
- **Ordering.** Hash move, good captures (MVV with capture history, SEE ≥ 0),
  killers, the counter-move, then quiets by butterfly history plus
  continuation history one and two plies back; bad captures last.
- **Correction history.** The static evaluation is corrected by how far it has
  recently been off for this pawn structure (16,384 entries per side, a
  depth-weighted running average of `search score − eval`).
- **Quiescence.** Captures and queen promotions with delta and SEE pruning;
  all moves when in check.
- **Draws** by repetition (any repetition in the search path or game counts),
  the fifty-move rule and insufficient material; the evaluation fades towards
  zero as the fifty-move counter rises.

Mate scores are kept relative to the node in the table and converted on the way
in and out.

### Time

`Limits::clock(time, increment, moves_to_go)` budgets a move: after 30 ms of
overhead, `soft = min(time/moves_to_go + 0.75 × increment, time/2)` with 25
moves assumed, `hard = min(3 × soft, 0.8 × time)`. The search starts no new
iteration after 60% of `soft` and stops outright at `hard`. Node limits work the
same way (`soft_nodes`, `nodes`).

### Threads

Lazy SMP: `search_parallel` starts helper searchers that share the main one's
table, network and stop flag, and search the same position. Only the main
thread keeps time and reports; its result is the answer. Natively the helpers
are `std::thread`s (the UCI `Threads` option). In the browser they are Web
Workers sharing the module's memory: `web/threads.rs` parks each helper on an
atomic until a search starts, and the page gives every helper its own stack and
thread-local storage before it runs (see [the web app](../../web/docs/README.md)).

## Evaluation

With a network embedded (`nets/kishi.nnue`, read by `include_bytes!`) the engine
evaluates with it; with an empty file it uses `hce.rs`. Both return centipawns
from the side to move's view. The network's accumulators live on a stack in the
searcher, one per ply, and are updated from the squares a move changed.

## Testing

```sh
cargo test --release -p kishi-engine                   # perft, FEN, hashing, SAN, SEE, mates
cargo test --release -p kishi-engine --features web    # also: every book line is legal
cargo run --release -p kishi-tools --bin kishi bench   # nodes and speed over 12 positions
```

To measure a change, build the old version as a UCI binary and play it with
`vs --engine <old binary> --nodes 10000` (see [the tools](../../tools/docs/README.md)).
