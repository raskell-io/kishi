# Engine protocol

Every engine module exports the same four functions and answers the same text
commands, so the page and the worker treat chess, shogi and Go alike.

## The ABI

```text
kishi_input(len) -> *mut u8     a buffer of `len` bytes to write the command into
kishi_command(len) -> usize     run the command; returns the reply's length
kishi_output() -> *const u8     the reply, UTF-8 JSON
kishi_seed(f64)                 seeds the randomness behind the lower levels
```

Imported: `env.kishi_now() -> f64`, milliseconds since any fixed point
(`performance.now()`).

Call `kishi_input` before taking a view of the memory: it may grow the memory,
which detaches older views. With the threaded build the memory is a
`SharedArrayBuffer`, which `TextDecoder` refuses, so copy the reply out first.
`web/engine.js` does both.

## Commands

| Command | Reply |
|---|---|
| `position startpos [moves m1 m2 …]` | the state after those moves, or `{"error": …}` |
| `position fen <fen> [moves …]` (chess), `position sfen <sfen> [moves …]` (shogi) | the same, from another start |
| `state` | the current state |
| `san <move>` | `{"san": "Nf3"}`: the move as the record writes it |
| `go level <1-8>` | a chosen move (not played) |
| `go ms <milliseconds>` | the same, at full strength for that long |
| `newgame` | `{}`: forget what the last game taught the search |
| `score` (Go only) | the count of a finished game |

Moves are written as each game's own engines write them: UCI for chess
(`e2e4`, `e7e8q`), USI for shogi (`7g7f`, `8h2b+`, `P*5e`), GTP for Go (`D4`,
`pass`).

## Replies

### State

```jsonc
// chess
{"fen": "…", "turn": "w", "legal": ["e2e4", …], "check": "e1", "result": ""}
// shogi
{"sfen": "…", "turn": "b", "legal": ["7g7f", "P*5e", …], "check": "", "result": ""}
// Go
{"board": "…361 chars…", "turn": "b", "legal": ["D4", …, "pass"], "last": "Q16",
 "ko": "", "captures": {"b": 0, "w": 2}, "komi": 7.5, "check": "", "result": ""}
```

`turn` is the side to move: `w`/`b` in chess, `b` (sente) / `w` (gote) in shogi,
`b`/`w` in Go. `check` names the checked king's square, or is empty. `result`
is empty while the game goes on, otherwise:

| Game | Results |
|---|---|
| Chess | `checkmate`, `stalemate`, `fifty`, `material`, `repetition` |
| Shogi | `checkmate`, `repetition` (fourfold), `moves` (400-move limit) |
| Go | `ended` (two passes in a row; ask for `score`) |

Go's `board` is the position row by row from the top: `x` black, `o` white, `.`
empty. Its `legal` list includes filling your own eyes (the search never does,
but a player may) and excludes repeating an earlier position (positional
superko).

### Chosen move

```jsonc
{"move": "e2e4", "san": "e4", "score": 23, "mate": null, "depth": 14,
 "nodes": 412345, "ms": 2491, "book": true}
```

`score` is in centipawns from the mover's side, `mate` the moves to mate (or
`null`), `book` present when the move came from the opening book. Go replies
with `winrate` (for the side to move) and `nodes` as simulations, and its
`move` may also be `resign`.

### Score (Go)

```jsonc
{"owner": "…361 chars of b, w or .…", "dead": "…361 chars of 0 or 1…",
 "margin": -7.5, "winner": "w"}
```

`margin` is black's area score minus white's, after komi, with the dead stones
taken off.

## Levels

| Level | Chess | Shogi | Go |
|---|---|---|---|
| 1 | every move scored at depth 1, softmax at 300 cp | depth 1, 300 cp | 300 simulations |
| 2 | depth 2, 170 cp | depth 2, 170 cp | 800 |
| 3 | depth 3, 100 cp | depth 2, 90 cp | 2,000 |
| 4 | depth 4, 55 cp | depth 3, 50 cp | 5,000 |
| 5 | depth 5, 28 cp | depth 4, 25 cp | 10,000 |
| 6 | depth 7, 12 cp | depth 5, 12 cp | 20,000 |
| 7 | 1.2M nodes, full strength, at most 3 s | 1 s | 1.5 s |
| 8 | 4M nodes, full strength, at most 6 s | 3 s | 5 s |

At levels 1 to 6 the chess and shogi engines score every legal move and pick
one at random, weighting each by `exp((score − best) / T)`: a high temperature
`T` makes mistakes likely, a low one rare, and a move into a seen mate is never
chosen. Chess levels 4 and above also follow a small opening book while the
game stays in it. Chess levels 7 and 8 are budgets in nodes rather than time,
so they play the same strength on every device; the search stops starting new
iterations at the budget, and stops outright at twice it or at the time cap,
whichever comes first.
