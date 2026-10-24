# The Go engine

`kishi-go` plays Go on the 19×19 board under Chinese rules (area scoring) with
komi 7.5. Without a trained network it searches the way the strong programs of
the late 2000s did, with Monte Carlo tree search. It is a library and, with the
`web` feature, the WebAssembly module the app plays (`web/go.wasm`).

| Module | Holds |
|---|---|
| `board.rs` | the board, groups and liberties, the rules, quick-game moves, scoring, GTP coordinates |
| `patterns.rs` | MoGo's 3×3 shape patterns as a lookup table |
| `mcts.rs` | the tree search, ownership, the final count, when to pass or resign |
| `web.rs` | the browser ABI and levels |

## Board

A 21×21 grid whose outer ring is a border, so a point's four neighbours are
always `p − 21`, `p − 1`, `p + 1`, `p + 21` without bounds checks. Stones of a
group form a circular list with one head; the head holds the group's size and
its liberties as a bitset of points (seven `u64`s). Placing a stone, merging
groups and capturing are a few word operations; a liberty count is a popcount.
The board also keeps a list of empty points, for picking random moves fast.

- **Legality.** A move is legal on an empty point that is not the ko point and
  either has an empty neighbour, joins a group with another liberty, or takes a
  group's last liberty. Suicide is illegal.
- **Ko.** A single stone that captures a single stone and is left in atari sets
  the ko point. Across the game, positional superko: the app and the root of the
  search refuse a move that repeats an earlier position (by Zobrist hash).
- **Eyes.** A point whose four neighbours are one colour's (or the edge), with
  at most one diagonal held by the other side (none on the edge), is that
  colour's eye. The search and the quick games never fill their own eyes.

## Search

Each simulation walks down the tree, adds one node, finishes the game with
quick moves, scores it, and backs the result up.

- **Selection.** RAVE: a child's value blends its own win rate with its
  "all moves as first" win rate, the latter weighted
  `β = √(1800 / (3n + 1800))` after `n` visits, so it guides the early visits
  and fades as real ones accumulate.
- **AMAF.** After a simulation, every child whose move the same player made
  later in that game (in the tree or the quick game) is credited with the
  result. This is what lets the search learn on a board this size.
- **Priors.** New children start with 24 virtual RAVE visits at a win rate from
  shape: the edge is poor, the third and fourth lines and the corners good early
  on; taking a group in atari or saving one's own, more urgent the bigger the
  group; a good 3×3 shape next to the last move; self-atari of two stones or
  more, bad; passing, poor until late.
- **Opening.** For the first eight moves the root only considers the 4-4, 3-4
  and 3-3 points of corners that are still empty; the search picks which.
- **Decision.** The most visited move. Around 24,000 simulations a second
  natively and 13,000 in the browser.

### Quick games

Not uniformly random (`Board::playout_move`):

1. If the last move left a group in atari, capture it; if it put one of ours
   in atari, extend it when that gives two liberties or more.
2. Otherwise, play one of the MoGo 3×3 patterns (hane, cuts, blocks, edge
   moves) among the eight points around the last move, at random.
3. Otherwise, any legal move that does not fill an own eye and, seven times in
   eight, does not put a group of two or more stones into atari.

A quick game ends after two passes or 3 × 361 moves, and is scored by area:
stones, plus empty points that touch one colour only.

### Patterns

`patterns.rs` holds the twelve patterns from Gelly, Wang, Munos and Teytaud
(2006), as Pachi writes them. Each point's eight neighbours (empty, black,
white or edge, two bits each) make a 16-bit code; a 65,536-entry table, built
once over all rotations, reflections and both colourings, says whether the
point is a good shape move.

## Passing, resigning and counting

`Searcher::decide` wraps the search:

- Kishi passes when the other side has just passed, or the search prefers
  passing, and its count of the position says it is winning. Otherwise it plays
  the best move other than pass.
- It resigns when its win rate is under 8% after move 120 and its count has it
  more than 30 points behind.

The count (`final_score`): a few hundred quick games from the position give
each point's ownership, from +1 (black every time) to −1 (white every time).
Stones whose owner is the other side more than 70% of the time are dead and
come off; then the position is scored by area (Tromp-Taylor) with komi. The app
shows the owners and the dead stones on the board.

## Strength

A beginner's opponent. Against GNU Go 3.8 at level 1, at a second a move, Kishi
wins 3 games in 10. Strong amateur play on 19×19 takes a trained policy and
value network; more simulations (threads) would help somewhat. Measure with
`go-vs` (see [the tools](../../tools/docs/README.md)).

## Testing

```sh
cargo test --release -p kishi-go                              # rules, ko, groups, random-game consistency, counting, passing
cargo run --release -p kishi-go --example think -- 3000       # the opening move and the simulation rate
cargo run --release -p kishi-go --example probe               # average quick-game results, to check for bias
```
