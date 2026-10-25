# Kishi as a study

Kishi is a playable app, and it is also an amateur research project. The
question it asks:

> How far can one person, with one laptop and the tools of 2026, take the
> methods behind thirty years of game AI, and end with an opponent people
> actually play?

The methods are the ones the papers describe: alpha-beta search with a learned
evaluation (NNUE), self-play as the training signal, Monte Carlo tree search,
and policy and value networks. The laptop is a MacBook Pro with an Apple M4.
The games are chess, shogi and Go, chosen because each breaks what worked on
the one before. The results are written up as a series of articles on
raskell.io, [*Learning to Play*](https://raskell.io/articles/learning-to-play/),
and everything the articles measure is recorded here first.

| | |
|---|---|
| [The notebook](notebook.md) | every run, dated, with its command, settings, result and what was learned from it, including the failures |
| [Strength](../strength.md) | the running table against Stockfish at lichess's levels |
| [`nets/LOG.md`](../../nets/LOG.md) | one line per trained generation: data, result against the previous generation at equal nodes and at equal time |
| [The tools](../../tools/docs/README.md) | the programs the experiments run: `datagen`, `train`, `arena`, `vs`, `shogi-vs`, `go-vs` |

## Rules

1. **One machine.** Every number is measured on the laptop below, and every
   experiment records how long it ran. No cloud, no borrowed GPU.
2. **Everything is reproducible from this repository.** Commands, seeds and
   settings are in the notebook. Data files are not in git (hundreds of
   megabytes), but the commands that regenerate them are. Self-play with
   several threads is not bit-for-bit deterministic; results are.
3. **Strength is measured against something external, at equal time.** A
   network that beats its predecessor at equal nodes has learned something; one
   that wins at equal milliseconds has earned its place, because inference costs
   speed. The anchors are Stockfish set up like lichess's levels (`vs
   --lichess N`), Fairy-Stockfish for shogi, and GNU Go.
4. **Honest instruments.** Paired games from the same openings with colours
   swapped, error bars at 95%, and a null test (an engine against itself) before
   trusting a new match setup.
5. **The app ships only what wins** at equal time on a phone.

## The machine

| | |
|---|---|
| Model | MacBook Pro, Apple M4 |
| Cores | 10: 4 performance, 6 efficiency |
| Memory | 16 GB |
| GPU | the M4's; unused so far, training runs on the CPU |
| Toolchain | Rust 1.99 (nightly for the threaded wasm build), wasm32 with `simd128`, `bulk-memory`, atomics |
| Opponents | Stockfish 19, Fairy-Stockfish 14, GNU Go 3.8, all from Homebrew |

Long runs go at low priority (`nice -n 20`) so the laptop stays usable; the
throughput overnight is the same.

## Experiments

| # | Question | Method | Status | Feeds |
|---|---|---|---|---|
| E1 | How strong is a classical engine with a hand-written evaluation? | `vs --lichess 4/6/7`, 20 games each | done 2026-10-22 | part 2 |
| E2 | Does a search change help, and can the instrument read zero? | `vs --engine <old> --nodes 10000 --games 600`, base vs base, then with correction history | done 2026-10-22 | part 2 |
| E3 | Does a network trained on self-play beat the evaluation that played the games? | `datagen` (gen 0), `train`, `arena` at 20,000 nodes | done 2026-10-22: +85 ± 32 | part 4 |
| E4 | What does the score-to-probability scale K do? | train with K = 400 and with K fitted (165), arena both | done 2026-10-22: −92 vs +85 | part 4 |
| E5 | Does it win at equal time? | `arena --ms 100` | done 2026-10-22: about even | parts 4, 5 |
| E6 | What do more generations do? | `scripts/generation.sh 2..5` with 8M positions each | gen 1 data 70% generated, paused 2026-10-25 | part 5, issues #1, #2 |
| E7 | Where does each generation land against lichess 7 and 8 on the phone's budget? | `vs --lichess 7/8 --kishi-threads 4`, 40 games | waiting on E6 | part 5, issue #3 |
| E8 | What is the speed tax of the network, exactly? | `kishi bench --hce` vs `--net`, one core, native and in the browser | to run | parts 3, 5, 9 |
| E9 | Does the same loop work for shogi, with king-relative features? | a shogi NNUE (HalfKP-style inputs), `shogi-datagen`, the same trainer, `shogi-vs --skill 0..10` | not started; the shogi crate has rules, search and a hand-written evaluation | part 6 |
| E10 | How strong is MCTS with patterns and RAVE on 19 × 19, without a network? | `go-vs --level 1..5`, 1 s a move | level 1 done 2026-10-24: 3/10; levels 2 to 5 to run | part 7 |
| E11 | What can a laptop-trained policy and value network add to that search? | a small residual network, self-play with MCTS visit counts as the policy target, `go-vs` | not started | part 8, issue #7 |
| E12 | How much strength does the browser lose to native, and threads regain? | `kishi bench` native vs wasm, 1 vs 4 workers on an iPhone | to run | part 9, issue #8 |
| E13 | Would training on the GPU (Metal) change what the laptop can do? | the trainer's inner loop in a Metal compute kernel or MLX, timed against the CPU | open question | part 5 or an appendix |

## The articles

| Part | Title | Draws on | State |
|---|---|---|---|
| 1 | Learning to Play | the book, the three games, the laptop, the first numbers | published 2026-10-25 |
| 2 | What the Network Has to Beat | E1, E2 | drafted |
| 3 | An Evaluation You Can Learn | the network's design, E8 | drafted |
| 4 | The Engine Plays Itself | E3, E4, E5 | drafted |
| 5 | Equal Nodes Is Not Equal Time | E5, E6, E7, E8 | waiting on the loop |
| 6 | The Same Loop, Nine by Nine | E9 | research |
| 7 | Where Search Stops Working | E10 | material in hand |
| 8 | Search as a Teacher | E11 | research |
| 9 | An Engine in Your Pocket | E12 | material in hand |
| 10 | Beyond the Board | side references | short coda |

Drafts live in the raskell.io repository under `content/articles/` with
`draft = true` until they are published.

## Side references

Two threads run beside the series and are kept there, as references and not as
subjects:

- Sebastian Mallaby, *The Infinity Machine: Demis Hassabis, DeepMind, and the
  Quest for Superintelligence* (Penguin, 2026): the human account of the
  AlphaGo years, the same loop at a scale no individual has.
- [AlphaCraft](https://github.com/broodwar-live/alphacraft) under
  broodwar.live: a learning agent for StarCraft: Brood War, the rung above Go
  (hidden information, real time, a stream of commands). Part 10 points at it.

## Reading

- Yu Nasu, *Efficiently Updatable Neural-Network-based Evaluation Functions
  for Computer Shogi* (2018).
- Kocsis and Szepesvári, *Bandit based Monte-Carlo Planning* (2006): UCT.
- Gelly and Silver, *Combining Online and Offline Knowledge in UCT* (2007): RAVE.
- Gelly, Wang, Munos and Teytaud, *Modification of UCT with Patterns in
  Monte-Carlo Go* (2006): the 3 × 3 patterns the Go engine's playouts use.
- Silver et al., *Mastering the game of Go with deep neural networks and tree
  search* (Nature, 2016); *Mastering the game of Go without human knowledge*
  (Nature, 2017); *A general reinforcement learning algorithm that masters
  chess, shogi, and Go through self-play* (Science, 2018).
- David Wu, *Accelerating Self-Play Learning in Go* (2019): KataGo.
- Vinyals et al., *Grandmaster level in StarCraft II using multi-agent
  reinforcement learning* (Nature, 2019): AlphaStar.
