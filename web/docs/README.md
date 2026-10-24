# The web app

Plain HTML, CSS and ES modules, no build step and no dependencies: what is in
`web/` is what is served. `make serve` builds the engines into `web/*.wasm` and
serves the folder with the headers the threaded engine needs
(`scripts/serve.py`).

| File | Holds |
|---|---|
| `index.html` | the page: header, board, player rows, moves, dialogs, the menu |
| `style.css` | everything visual: tokens, boards, pieces, dialogs |
| `app.js` | the shell: game state, rendering, the worker, dialogs, saving |
| `games/chess.js`, `games/shogi.js`, `games/go.js` | one module per game (the contract below) |
| `board.js`, `shogi-board.js`, `go-board.js` | the three boards: drawing and input |
| `engine.js` | loading an engine module, single-threaded or threaded |
| `worker.js` | the search worker |
| `helper.js` | a helper thread of the threaded chess engine |
| `sw.js` | the service worker: offline cache, cross-origin isolation |
| `pieces/`, `fonts/` | Chessnut pieces (Apache 2.0); Geist, Geist Mono and the shogi kanji (OFL) |

## The shell

`app.js` knows nothing about any game's rules. It holds the current game as
`{ moves, sans, color, level, end }` (moves in the engine's notation, `sans` as
the record writes them, `color` the side you play), renders it, and asks:

- the **rules engine** (the game's wasm on the main thread) for legal moves,
  check and results, and for each move's notation;
- the **game module** for the board, how input becomes a move, what the player
  rows show, how the game ends and how it is exported;
- the **worker** for Kishi's moves.

It also draws what every game shares: the player rows, the move list (paired
for chess, numbered singly for shogi and Go), history navigation with the
review banner, the result card and bar, the new-game sheet, the game switcher,
the menu with settings and engine details, and sounds.

## The game module contract

A game is an object with these members (see `games/go.js` for the shortest
complete one):

| Member | Type | Meaning |
|---|---|---|
| `id`, `name` | string | `"go"`, `"Go"` |
| `wasm` | string | the engine module, e.g. `"go.wasm"` |
| `threaded` | string or null | a threaded build of it, if any (`"kishi-mt.wasm"`) |
| `first` | string | the side that moves first, as the engine's `turn` writes it |
| `sides` | `{side: name}` | both sides, e.g. `{ b: "black", w: "white" }` |
| `sideLabels` | `{ first, second }` | the new-game buttons |
| `pairs` | bool | number moves in pairs (chess) or singly |
| `exportLabel` | string | the menu's copy button |
| `reasons` | `{result: text}` | how to say each engine `result` |
| `createBoard(el, onMove)` | → board | build the board in `el`; call `onMove(from, to)` for a move |
| `show(board, view)` | | draw `view = { state, lastMove, dests, orientation, animate, end, live }` |
| `dests(legal)` | → `Map(from → Set(to))` | the moves the board may offer |
| `choose(board, legal, from, to, turn)` | → `Promise<move or null>` | the move for a tap; ask about promotion here |
| `sideDetail(state, color, away)` | → HTML | the right side of a player's row (captures, hand, komi) |
| `isCapture(san)` | → bool | which click to play |
| `resultCode(winner, end)` | → string | `1-0`, `sente wins`, `W+7.5` |
| `export(game)` | → string | PGN, a shogi record with its USI line, SGF |
| `icon(side, kind)` | → HTML, optional | a piece or stone for dialogs and the switcher |
| `wireSide(el, board, yours)` | optional | connect a player's row to the board (shogi's hand) |
| `canPass` | bool, optional | show the Pass button |
| `finish(rules, state)` | → end, optional | count a finished game (Go asks its engine to `score`) |
| `evalText(info)` | → string, optional | how to show Kishi's estimate |

### Adding a game

1. A Rust crate with a `web` feature whose `web.rs` answers
   [the protocol](../../docs/protocol.md); add it to the workspace and a target
   to `Makefile`'s `build`.
2. A board class with `set(view)`, `orientation`, `onInteract` and `destroy()`.
3. A module in `games/` filling the contract; import it in `app.js` and add it
   to `GAMES`. The menu, switcher and new-game sheet list games from there.
4. Add the new files to `SHELL` in `sw.js` and bump `CACHE`.

## The boards

All three share the stone board colours (`--light-sq`, `--dark-sq`, set by the
board theme), the vermilion last-move tint and the mono coordinates, and draw
pieces as absolutely positioned elements moved with `transform`.

- **Chess** (`board.js`): Chessnut SVG pieces, tap or drag, legal-move dots,
  pieces slide from where they were.
- **Shogi** (`shogi-board.js`): `komaSvg` draws every piece, an ivory pentagon
  in a bold ink line on a wood edge, lettered in the bundled Noto Sans JP
  subset (`fonts/kishi-koma.woff`, the fifteen piece characters). Pieces in
  hand are buttons in the player rows that call `selectDrop`. Promotion is two
  pieces stacked on the target square.
- **Go** (`go-board.js`): an SVG grid with star points, `stoneSvg` stones in the
  same style as the shogi pieces. With a mouse a click places a stone; by touch
  the first tap shows it with a crosshair and the second places it. After the
  count, owners and dead stones are drawn on the board.

## The worker and threads

`worker.js` receives `{type: "init", wasm, threaded}`, loads the engine, and
answers `{type: "search", token, moves, level}` with
`{type: "bestmove", token, …the engine's reply}`. The shell ignores replies
whose token is stale; to stop a search it terminates the worker and starts a
new one.

For chess, `engine.js` tries the threaded build first. That needs
`crossOriginIsolated` (see below). It creates a shared `WebAssembly.Memory`,
instantiates the module, then starts up to three `helper.js` workers. For each
it allocates a stack (1 MB) and a thread-local storage block from the module's
own allocator (`kishi_alloc`) and posts `{module, memory, stackTop, tls}`. The
helper instantiates the same module on the same memory, points its
`__stack_pointer` at its stack, calls `__wasm_init_tls`, and enters
`kishi_helper_run`, which never returns. If any of that fails, the worker falls
back to `kishi.wasm` on one thread.

## Offline, isolation and updates

`sw.js` precaches the files listed in `SHELL` and answers every same-origin
request from the cache first, refreshing it in the background. A change shows
on the second visit after a deploy; bump `CACHE` when files are added or
renamed. Every response it serves carries `Cross-Origin-Opener-Policy:
same-origin` and `Cross-Origin-Embedder-Policy: require-corp`, which makes the
page cross-origin isolated on hosts that cannot send headers (GitHub Pages). On
the first visit the app registers the worker and reloads once
(`isolate()` in `app.js`); if the browser still will not isolate, it plays
single-threaded.

## What is saved

`localStorage`, per browser:

| Key | Holds |
|---|---|
| `kishi.settings.v1` | `{ game, theme, sound, showEval }` |
| `kishi.game.v1` | the chess game (the key predates the other games) |
| `kishi.game.v1.shogi`, `kishi.game.v1.go` | the shogi and Go games |

A saved game whose moves no longer replay (after a rules change) is quietly
replaced by a new one.

## Style

Tokens at the top of `style.css`: ink `--ink`, raised surfaces `--raise`,
`--raise-2`, text `--text`, `--strong`, `--muted`, the one accent `--accent`
(vermilion), Geist for text and Geist Mono for moves, coordinates and numbers.
Board themes (`stone`, `slate`, `wood`, `green`) set `data-board` on the root
and redefine the square colours. Motion respects `prefers-reduced-motion`.
