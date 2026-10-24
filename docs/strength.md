# Strength

How Kishi's chess engine does against Stockfish set up like lichess's computer
levels (fishnet's skill level, depth limit and think time per level; `vs
--lichess N`). Kishi plays at the time or node budget the app gives the level;
the phone runs four threads, so matches since the node budgets use
`--kishi-threads 4`.

Error bars are 95% intervals; 20 games give roughly ± 200 Elo, 40 games ± 140.

## Hand-written evaluation (generation 0)

Measured 2026-10-22, Stockfish 19, 20 games each, Kishi on one thread.

| Opponent | Kishi's time | Score | Elo |
|---|---|---|---|
| lichess level 4 | 200 ms | 19 / 20 | +511 ± 646 |
| lichess level 6 | 500 ms | 13 / 20 | +108 ± 165 |
| lichess level 7 | 500 ms | ½ / 20 | −636 ± 579 |

## Networks

Filled in as generations finish (#1, #3): levels 6, 7 and 8 at the app's
budgets for levels 7 and 8, 40 games, four threads.

| Generation | vs level 6 | vs level 7 | vs level 8 |
|---|---|---|---|
| 1 | | | |
