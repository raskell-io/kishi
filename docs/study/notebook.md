# Notebook

Every run that produced a number, newest last. Each entry: what was asked, the
command, the result, what was learned. Failures stay in. Times are wall-clock
on the laptop described in [the study](README.md); "cores" is the thread
count given to the tool.

Elo differences are from paired games (each random opening played twice with
colours swapped); error bars are 95% intervals.

## 2026-10-22 · Chess baseline against lichess's levels (E1)

Stockfish 19 set up as fishnet plays each level (skill, depth, time); Kishi
with the hand-written evaluation (PeSTO), one thread, at the time its app gives
the matching level. 20 games a level.

```text
vs --engine stockfish --lichess 4 --ms 200 --games 20
Kishi (200 ms/move) vs lichess level 4: +19 =0 -1   elo +511.5 ± 646.2
vs --engine stockfish --lichess 6 --ms 500 --games 20
Kishi (500 ms/move) vs lichess level 6: +12 =2 -6   elo +107.5 ± 165.2
vs --engine stockfish --lichess 7 --ms 500 --games 20
Kishi (500 ms/move) vs lichess level 7: +0 =1 -19   elo -636.4 ± 578.9
```

Learned: between level 6 and level 7 on one core at half a second. 20 games
give ± 200 Elo at best; the table is a placement, not a measurement. Redo
with 40 games and the phone's four threads as networks arrive (E7).

## 2026-10-22 · The instrument reads zero; correction history (E2)

Same openings, both sides 10,000 nodes a move, 600 games. First the engine
against its own binary, then with pawn-structure correction history against
the engine without it.

```text
vs --engine bin/kishi-base --nodes 10000 --games 600
Kishi (10000 nodes/move) vs bin/kishi-base at 10000 nodes:   +211 =181 -208  elo +1.7 ± 23.3
vs --engine bin/kishi-nocorr --nodes 10000 --games 600
Kishi (10000 nodes/move) vs bin/kishi-nocorr at 10000 nodes: +219 =180 -201  elo +10.4 ± 23.3
```

Learned: the null test passes (+1.7 within ± 23). Correction history is
probably positive and not proven at this budget; kept. Time per 600-game match
not recorded (to measure).

## 2026-10-22 · Generation 0 self-play data (E3)

The hand-written evaluation plays itself: random openings of 8 or 9 plies
kept if within 250 cp, about 5,000 nodes a move, quiet positions recorded with
the search score, results written back at the end of each game. 10 threads.

```text
datagen --brain hce --out data/gen0.bin --positions 10000000 --threads 10
5579892 positions, 57046 games (+19383 =17752 -19911), 1795 pos/s
```

Stopped after 52 minutes at 5,585,152 records (178 MB), enough for a first
network. Result split: 34% white wins, 31% draws, 35% black wins. Throughput
held at 1,790 to 1,900 positions a second throughout.

## 2026-10-22 · Generation 1 with K = 400: loses (E4)

Trained on `data/gen0.bin` with the score-to-probability scale set to 400 (the
Elo formula's constant, which has nothing to do with centipawns). 30 epochs,
Adam, cosine learning rate from 0.001, batch 16,384, `wdl` 0.3. Training and
validation loss fell normally; quantisation error about 2 cp.

```text
arena --a nets/gen1.nnue --b hce --games 400 --nodes 20000
nnue vs hce: +113 =71 -216  elo -91.5 ± 31.8
```

Two more variants on the same data (a 256-wide hidden layer, `data/gen1-256.nnue`,
and a different epoch count) lost by 45.4 ± 44.1 and 88.7 ± 46.2.

Learned: nothing the trainer prints could see this. The search's pruning
margins are in PeSTO's centipawns; a network trained with the wrong K speaks
on another scale and every margin misfires. The K = 400 training log was not
kept (to redo once with `--k 400` for the article).

## 2026-10-22 · Generation 1 with K fitted = 165: wins at equal nodes (E3, E4)

`train` now fits K to the data (the K under which `sigmoid(score / K)` best
predicts the recorded results): 165 on generation 0's games. Same data, same
network, same settings otherwise.

```text
train --data data/gen0.bin --out nets/gen1.nnue --epochs 30
arena --a nets/gen1.nnue --b hce --games 400 --nodes 20000
nnue vs hce: +217 =62 -121  elo +85.0 ± 32.2
```

A 176 Elo swing from one constant. Training time not recorded (to measure on
generation 2; `generation.sh` now logs it).

## 2026-10-22 · Generation 1 at equal time: even (E5)

`arena --ms 100`, generation 1 against the hand-written evaluation. The
network costs about 2.5 × per node (see the throughput below), so at equal time
the table searches 2.5 × the positions.

Three matches, 200 to 300 games each, landed at −41 ± 47, −28 ± 50 and
+22 ± 49: even within the error bars. Which settings each used was not logged
(to rerun one at 100 ms with 300 games for the article).

Decision: the app keeps the hand-written evaluation (`nets/kishi.nnue` stays
empty). A network ships when it wins at equal time.

## 2026-10-24 · Shogi and Go baselines (E10)

Shogi, hand-written evaluation, against Fairy-Stockfish 14 at 500 ms a move
each, 20 games a level: 20/20 at skill level 0, 2/20 at skill level 10.

Go, MCTS with RAVE and MoGo patterns, no network, against GNU Go 3.8 level 1,
Chinese rules, komi 7.5, 1 s a move, 10 games: 3 wins. About 24,000 playouts a
second natively, 13,000 in the browser on one thread.

Learned: the shogi engine is a fair club-level opponent and far from strong
play; the Go engine is a beginner's opponent, as expected of 2008-era methods
on 19 × 19 at one second. Levels 2 to 5 of GNU Go still to run.

## 2026-10-24 · Levels 7 and 8 become node budgets

The app's top two levels now search 1.2M and 4M nodes (capped at 3 s and 6 s)
instead of a fixed time, so a phone with four threads thinks as deep as a
laptop. Matches against lichess levels should now use `--kishi-threads 4`.

## 2026-10-24 → 25 · Generation 1 self-play data (E6)

Generation 1 plays itself with `nets/gen1.nnue`, 8 threads, target 8M
positions, chained to `generation.sh 2` and `3`.

```text
datagen --brain nets/gen1.nnue --out data/gen1.bin --positions 8000000 --seed 1000 --threads 8
5619216 positions, 58780 games (+20850 =16709 -21221), 721 pos/s
```

Throughput 720 positions a second against 1,790 with the table on ten threads:
the 2.5 × tax of inference, measured in self-play. Result split 35/28/36:
fewer draws than generation 0's games.

Paused on 2026-10-25 with SIGSTOP at 5,623,296 records (70%) because the
laptop was needed; the run had taken the machine for itself. Learned: run
long jobs with `nice -n 20`. The file is append-only and resumes where it
stopped; the chain to generations 2 and 3 is waiting behind it.

## 2026-10-25 · Search depth and speed on one core (E8, hand-written evaluation)

`kishi` over UCI, one thread, `go movetime 500`, hand-written evaluation:

```text
startpos:                 depth 20, 1,565,303 nodes, 3.37M nodes/s
middlegame (move 8, QGD): depth 17,   934,163 nodes, 3.00M nodes/s
kishi bench (12 positions, depth 12): 674,843 nodes, 3.68M nodes/s
```

Learned: at the app's half-second budget the engine sees 17 to 20 plies on one
core. The same figures with `--net nets/gen1.nnue` are still to be taken; the
2.5 × per-node tax so far comes from self-play throughput only.
