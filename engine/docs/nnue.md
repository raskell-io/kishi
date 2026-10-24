# The network

Kishi's evaluation network is a small NNUE ("efficiently updatable neural
network") in the style of modern engines, trained by `tools/src/bin/train.rs`
on self-play games (see [the tools](../../tools/docs/README.md)).

## Architecture

```text
 768 inputs  ──►  512 (shared weights)  ──►  SCReLU ─┐
 (side to move's view)                               ├─► 8 output buckets ─► 1
 768 inputs  ──►  512 (shared weights)  ──►  SCReLU ─┘
 (the other side's view)
```

- **Inputs.** One per (colour relative to the viewer, kind, square): 2 × 6 × 64
  = 768. Seen from white, a white knight on g1 is `0 × 384 + 1 × 64 + 6`; seen
  from black the board is flipped (`square ^ 56`) and colours swap, so the same
  first layer serves both sides.
- **Hidden layer.** 512 per side (`HIDDEN` in `nnue.rs`), the same weights for
  both views. Kept as an accumulator per view and updated incrementally: a quiet
  move subtracts one row and adds one; a capture subtracts two; castling moves
  two pieces. The update is fused, one pass per view over the changed rows.
- **Activation.** Squared clipped ReLU: `clamp(x, 0, 1)²`.
- **Output.** One of eight output layers, chosen by the number of pieces
  (`(pieces − 2) / 4`, at most 7), reads the side to move's half then the
  other's: 1,024 weights and a bias.

The evaluation in centipawns is the output times 400 (`SCALE`).

## Quantisation

The first layer is integer: weights and biases are multiplied by `QA = 255`
and stored as i16, so an accumulator is i16 and the clamp is to `[0, 255]`. The
output layer stays f32: rounding its 1,024 weights to i16 cost more than 10
centipawns on average, while f32 keeps the integer and float networks within
about 2 centipawns of each other (the trainer checks this after every run). The
square is exact in i32, then one f32 multiply-add per input, in sixteen lanes
so it vectorises with SIMD (NEON natively, simd128 in the browser):

```text
eval = (Σ clamp(a, 0, 255)² × w / 255² + b) × 400
```

## File format

`nets/*.nnue`, little-endian, written by `Network::to_bytes` and read by
`Network::from_bytes`:

| Bytes | Content |
|---|---|
| 4 | magic `KSHI` |
| 4 | hidden size, u32 (512) |
| 4 | bucket count, u32 (8) |
| 768 × 512 × 2 | first-layer weights, i16, feature-major |
| 512 × 2 | first-layer biases, i16 |
| 8 × 1024 × 4 | output weights, f32, per bucket: side to move's 512, then the other's |
| 8 × 4 | output biases, f32 |

820,268 bytes in all. A network whose sizes differ from the engine's constants
is refused, not misread.

## Which network ships

The chess build embeds `nets/kishi.nnue`. It is empty today, so the app plays
with the hand-written evaluation: generation 1 (`nets/gen1.nnue`) beats it by
85 Elo at equal nodes, but is about even at equal time because inference makes
each node about 2.5 times slower. A network goes in once it wins at equal time.
`nets/LOG.md` records each generation.
