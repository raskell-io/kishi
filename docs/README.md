# Kishi documentation

Kishi is three game engines written in Rust, compiled to WebAssembly, and a
small web app that plays them on your device. These pages explain how it is
built and how to change it.

| Read | For |
|---|---|
| [Architecture](architecture.md) | How the pieces fit: crates, wasm builds, the worker, the page |
| [Engine protocol](protocol.md) | The text commands every engine answers, and their JSON replies |
| [Brand](brand/README.md) | The seal, the wordmark, colours and type, and how they are generated |
| [The study](study/README.md) | Kishi as a research object: the questions, the experiments, the notebook of every run, and the article series |
| [Chess engine](../engine/docs/README.md) | Board, move generation, search, evaluation, threads |
| [The network](../engine/docs/nnue.md) | NNUE architecture, file format, quantisation |
| [Shogi engine](../shogi/docs/README.md) | Rules, notation, search and evaluation for shogi |
| [Go engine](../go/docs/README.md) | Board, Monte Carlo tree search, patterns, counting |
| [Tools](../tools/docs/README.md) | UCI front end, self-play training, matches against other engines |
| [Web app](../web/docs/README.md) | The shell, game modules, boards, worker, offline and threads |

## Quick start

```sh
make serve     # build every engine to web/*.wasm and serve web/ on :8647
make test      # all Rust tests: perft, rules, search, counting, trainer
make bench     # native chess search speed
```

Every push to `main` runs the tests, builds the site and deploys it to
https://kishi.raskell.io (`.github/workflows/pages.yml`).

## Layout

```text
engine/   kishi-engine   chess: rules, search, NNUE, the web ABI, threads
shogi/    kishi-shogi    shogi: rules, search, evaluation, the web ABI
go/       kishi-go       Go: board, MCTS, patterns, counting, the web ABI
tools/    kishi-tools    native binaries: UCI, datagen, train, arena, vs, shogi-vs, go-vs
web/                     the app: HTML, CSS, JS modules, wasm builds, service worker
nets/                    trained networks (kishi.nnue is embedded in the chess build)
scripts/                 brand generator, social card, dev server, one RL generation
docs/                    this documentation, brand files, screenshots
```
