# The shogi engine

`kishi-shogi` plays shogi with the same kind of search as the chess engine. It
is a library and, with the `web` feature, the WebAssembly module the app plays
(`web/shogi.wasm`).

| Module | Holds |
|---|---|
| `position.rs` | the board and hands, SFEN, moves and drops, the rules, USI and Western notation, `perft` |
| `eval.rs` | the hand-written evaluation |
| `search.rs` | iterative deepening PVS and quiescence |
| `web.rs` | the browser ABI and levels |

## Board

A 9×9 mailbox, row-major from gote's back rank and from the 9-file, as sente
sees the board: index `row × 9 + col`, so `7g` is row 6, column 2. Sente (black,
who moves first) sits at the bottom and moves up; gote's moves are sente's
mirrored. A piece is its kind (1 pawn … 8 king, 9–14 the promoted kinds) plus 16
for gote. Each side has a hand of seven kinds with counts.

Moves are 16 bits: destination (7), origin (7; for a drop, 81 + the dropped
kind) and a promotion bit. The hash is Zobrist over pieces, hand counts and the
side to move.

## Rules

- **Movement.** Each kind is a list of single steps and a list of sliding
  directions (`movement` in `position.rs`): gold-movers share one list; the
  horse is the bishop plus orthogonal steps, the dragon the rook plus diagonal
  ones.
- **Promotion.** Optional when a move starts or ends in the far three ranks,
  forced where the piece could never move again (a pawn or lance on the last
  rank, a knight on the last two).
- **Drops.** On any empty square, except a pawn or lance on the last rank, a
  knight on the last two, or a pawn on a file where its side already has an
  unpromoted pawn (nifu).
- **Uchifuzume.** A pawn drop that delivers mate is illegal: after a pawn drop
  that gives check, the opponent must have a legal reply.
- **The end.** No legal move is a loss, whether in check or not. Four times the
  same position is a draw (sennichite); the perpetual-check exception is not
  modelled. Games are called a draw after 400 moves.

Move generation is checked by `perft` against published counts: 30, 900,
25,470, 719,731 and 19,861,490 from the start, and 207, 28,684 and 4,809,015
from the drop-heavy "matsuri" position. In 52 games against Fairy-Stockfish,
every move it played was legal by Kishi's generator.

## Notation

The record is in Western notation, as on lishogi: piece letter, origin only
when another piece of the same kind could reach the square, `-` a move, `x` a
capture, `*` a drop, then `+` to promote or `=` to decline where promotion was
possible: `P-7f`, `Bx2b+`, `G4i-5h`, `S*5e`. The protocol uses USI (`7g7f`,
`8h2b+`, `P*5e`).

## Search

The same plan as chess (see [the chess engine](../../engine/docs/README.md)),
and the same transposition table: PVS with aspiration windows, null-move
pruning, reverse futility pruning, late move pruning and reductions, killers and
history (indexed by origin, a drop's origin included, and destination), a
quiescence search over captures with delta pruning, and check extensions. Any
earlier occurrence of a position in the search path or recent game counts as a
draw. About 2.3 million nodes a second natively; depth 16 from the start in 3
seconds.

## Evaluation

From the side to move's view, in centipawns (a pawn on the board is 100):

- **Material.** Board values from 100 (pawn) to 1,300 (dragon); a piece in hand
  is worth a little more than on the board, since a drop can land anywhere.
- **King shelter.** A gold or silver within two squares of its own king, +24.
  A king that leaves its back ranks pays 18 a rank.
- **Pressure.** Every piece within three squares of the enemy king, more the
  closer it is and the stronger the piece (promoted pieces count most).
- **Mobility.** 4 per square a bishop, rook, horse or dragon reaches.
- **Pawns.** 4 per rank advanced.

## Strength

Against Fairy-Stockfish 14 at 0.5 s a move: 20 of 20 at skill level 0, 2 of 20
at skill level 10. Play it yourself with `shogi-vs` (see
[the tools](../../tools/docs/README.md)).

## Testing

```sh
cargo test --release -p kishi-shogi                          # perft, SFEN, hashing, uchifuzume, notation, search
cargo run --release -p kishi-shogi --example think -- 3000   # a search from the start: move, depth, speed
```
