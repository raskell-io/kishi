// The shell: you against Kishi at one of eight levels, in one of several games.
// Each game module (games/*.js) supplies its board, how a tap becomes a move,
// what the player rows show, and how a game is exported.
import { loadEngine, positionCommand } from './engine.js';
import chess from './games/chess.js';
import shogi from './games/shogi.js';

const GAMES = { chess, shogi };
const LEVEL_NAMES = ['', 'Novice', 'Beginner', 'Casual', 'Intermediate', 'Club', 'Strong', 'Expert', 'Maximum'];
const SETTINGS_KEY = 'kishi.settings.v1';
// Chess keeps the key it had before there were other games.
const saveKey = (id) => (id === 'chess' ? 'kishi.game.v1' : `kishi.game.v1.${id}`);

const $ = (sel) => document.querySelector(sel);

let settings = { showEval: false, sound: true, theme: 'stone', game: 'chess', ...load(SETTINGS_KEY) };
document.documentElement.dataset.board = settings.theme;
let G = GAMES[settings.game] ?? chess; // the current game module
let rules; // that game's engine on the main thread, for legal moves and notation
let board;
let worker = null;
let token = 0;
let thinking = false;
let sentAt = 0;
let engineThreads = 1;
let game; // { moves, sans, color, level, end }
let viewPly = null; // the ply on display while browsing history; null follows the game
let lastInfo = null;
let flipped = false;

function load(key) {
  try {
    return JSON.parse(localStorage.getItem(key));
  } catch {
    return null;
  }
}

function save(key, value) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {}
}

const freshGame = () => ({ moves: [], sans: [], color: G.first, level: 4, end: null });
const second = () => Object.keys(G.sides).find((s) => s !== G.first);
const other = (side) => (side === G.first ? second() : G.first);

function loadGame() {
  const g = load(saveKey(G.id));
  return g && Array.isArray(g.moves) ? g : freshGame();
}

const saveGame = () => save(saveKey(G.id), game);

// ---- sound ------------------------------------------------------------------

let audio = null;
function unlockAudio() {
  if (!audio && settings.sound) {
    try {
      audio = new AudioContext();
    } catch {}
  }
  audio?.resume?.();
}

function click(capture) {
  if (!settings.sound || !audio) return;
  const t = audio.currentTime;
  const len = capture ? 0.09 : 0.06;
  const buffer = audio.createBuffer(1, Math.ceil(audio.sampleRate * len), audio.sampleRate);
  const data = buffer.getChannelData(0);
  for (let i = 0; i < data.length; i++) {
    const k = i / data.length;
    data[i] = (Math.random() * 2 - 1) * Math.pow(1 - k, capture ? 3 : 5);
  }
  const src = audio.createBufferSource();
  src.buffer = buffer;
  const filter = audio.createBiquadFilter();
  filter.type = 'bandpass';
  filter.frequency.value = capture ? 900 : 1500;
  filter.Q.value = 1.2;
  const gain = audio.createGain();
  gain.gain.value = capture ? 0.9 : 0.6;
  src.connect(filter).connect(gain).connect(audio.destination);
  src.start(t);
}

// ---- engine -----------------------------------------------------------------

function startWorker() {
  worker = new Worker(new URL('worker.js', import.meta.url), { type: 'module' });
  worker.postMessage({ type: 'init', wasm: G.wasm, threaded: G.threaded });
  const id = G.id;
  worker.onmessage = ({ data }) => {
    if (data.type === 'ready') engineThreads = data.threads;
    if (data.type !== 'bestmove' || data.token !== token || id !== G.id) return;
    // Instant replies feel abrupt; give the opponent a moment.
    setTimeout(() => {
      if (data.token !== token) return;
      thinking = false;
      lastInfo = data;
      play(data.move);
    }, Math.max(0, 350 - (performance.now() - sentAt)));
  };
}

function stopThinking() {
  token++;
  if (thinking) {
    worker?.terminate();
    startWorker();
    thinking = false;
  }
}

function think() {
  thinking = true;
  const t = ++token;
  sentAt = performance.now();
  worker.postMessage({ type: 'search', token: t, moves: game.moves, level: game.level });
  render();
}

// ---- game -------------------------------------------------------------------

const positionAt = (ply) => rules.command(positionCommand(game.moves.slice(0, ply)));
const liveState = () => positionAt(game.moves.length);
const enginesTurn = (state) => !game.end && state.turn !== game.color;

async function onBoardMove(from, to) {
  if (viewPly !== null) return;
  const state = liveState();
  const move = await G.choose(board, state.legal, from, to, state.turn);
  if (move) play(move);
  else render();
}

function play(move) {
  rules.command(positionCommand(game.moves));
  const { san } = rules.command(`san ${move}`);
  if (!san) return;
  game.moves.push(move);
  game.sans.push(san);
  const state = liveState();
  if (state.result) game.end = { reason: state.result, winner: state.result === 'checkmate' ? other(state.turn) : null };
  saveGame();
  click(G.isCapture(san));
  viewPly = null;
  render(true);
  if (enginesTurn(state)) think();
}

function newGame(choice, level) {
  stopThinking();
  worker.postMessage({ type: 'newgame' });
  const color = choice === 'first' ? G.first : choice === 'second' ? second() : Math.random() < 0.5 ? G.first : second();
  game = { moves: [], sans: [], color, level, end: null };
  flipped = false;
  lastInfo = null;
  viewPly = null;
  saveGame();
  render();
  if (enginesTurn(liveState())) think();
}

function takeback() {
  if (!game.moves.length) return;
  stopThinking();
  // Undo Kishi's reply, if any, and your last move, so it is your turn again.
  while (game.moves.length) {
    const ply = game.moves.length - 1;
    const yours = (ply % 2 === 0) === (game.color === G.first);
    game.moves.pop();
    game.sans.pop();
    if (yours) break;
  }
  game.end = null;
  lastInfo = null;
  viewPly = null;
  saveGame();
  render();
  // Back at the start as the second player: Kishi opens again.
  if (enginesTurn(liveState())) think();
}

function resign() {
  if (game.end || !game.moves.length) return;
  stopThinking();
  game.end = { reason: 'resign', winner: other(game.color) };
  saveGame();
  render();
}

async function share() {
  const text = G.export(game);
  try {
    if (navigator.share) await navigator.share({ title: `Kishi ${G.name.toLowerCase()} game`, text });
    else {
      await navigator.clipboard.writeText(text);
      toast('Copied');
    }
  } catch {}
}

function toast(message) {
  const t = $('#toast');
  t.textContent = message;
  t.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => (t.hidden = true), 1600);
}

/** Switches to another game: its engine, its board, and its saved game. */
async function switchGame(id) {
  if (id === G.id) return;
  stopThinking();
  // Load the new engine first: until it is ready, the old game keeps rendering.
  const next = GAMES[id];
  const nextRules = await loadEngine(new URL(next.wasm, import.meta.url));
  worker?.terminate();
  board?.destroy?.();
  G = next;
  rules = nextRules;
  settings.game = id;
  save(SETTINGS_KEY, settings);
  setupBoard();
  game = loadGame();
  if (positionAt(game.moves.length).error) game = freshGame();
  flipped = false;
  lastInfo = null;
  viewPly = null;
  startWorker();
  render();
  if (enginesTurn(liveState())) think();
}

function setupBoard() {
  const old = $('#board');
  const el = document.createElement('div');
  el.id = 'board';
  old.replaceWith(el);
  board = G.createBoard(el, onBoardMove);
  board.onInteract = unlockAudio;
  // Selecting on the board changes which hand piece is lit.
  board.onSelect = () => renderPlayers(positionAt(viewPly ?? game.moves.length));
  document.body.dataset.game = G.id;
  $('#game-name').textContent = G.id === 'chess' ? '' : G.name.toLowerCase();
}

// ---- rendering --------------------------------------------------------------

function formatScore(info) {
  if (!info) return '';
  // Scores are from Kishi's side; show them from the first player's.
  const sign = game.color === G.first ? -1 : 1;
  if (info.mate !== null) return `#${info.mate * sign}`;
  const s = (info.score * sign) / 100;
  return (s > 0 ? '+' : '') + s.toFixed(1);
}

function renderPlayer(el, color, isEngine, state) {
  let name, meta;
  if (isEngine) {
    name = 'Kishi';
    meta = `level ${game.level}`;
    if (thinking) meta = '<span class="breath"></span>thinking';
    else if (settings.showEval && lastInfo) {
      meta += ` · <span class="score">${lastInfo.book ? 'book' : `${formatScore(lastInfo)} d${lastInfo.depth}`}</span>`;
    }
  } else {
    name = 'You';
    meta = G.sides[color];
  }
  el.innerHTML = `<div class="who"><span class="turn"></span><span class="name">${name}</span><span class="meta">${meta}</span></div>${G.sideDetail(state, color, el.id === 'top')}`;
  G.wireSide?.(el, board, !isEngine && viewPly === null && !game.end && !thinking);
}

function renderPlayers(state) {
  const orientation = flipped ? other(game.color) : game.color;
  const top = other(orientation);
  renderPlayer($('#top'), top, top !== game.color, state);
  renderPlayer($('#bottom'), orientation, orientation !== game.color, state);
  const live = viewPly === null;
  $('#top').classList.toggle('to-move', live && !game.end && state.turn === top);
  $('#bottom').classList.toggle('to-move', live && !game.end && state.turn !== top);
}

/** The engine section of the menu: what Kishi is, and what its last search did. */
function renderEngineInfo() {
  const rows = [
    ['game', G.name.toLowerCase()],
    ['threads', `${engineThreads}${engineThreads > 1 ? ' (Web Workers, shared memory)' : ''}`],
    ['build', 'rust → wasm · simd128'],
    ['eval', G.id === 'chess' ? 'hand-written (network in training)' : 'hand-written'],
    ['hash', '32 MB'],
  ];
  if (lastInfo && !lastInfo.book && lastInfo.nodes) {
    const nodes = lastInfo.nodes >= 1e6 ? `${(lastInfo.nodes / 1e6).toFixed(1)}M` : `${Math.round(lastInfo.nodes / 1e3)}k`;
    const time = lastInfo.ms < 1000 ? `${lastInfo.ms} ms` : `${(lastInfo.ms / 1000).toFixed(1)} s`;
    rows.push(['last search', `depth ${lastInfo.depth} · ${nodes} nodes · ${time}`]);
  }
  $('#engine-info').innerHTML = rows.map(([k, v]) => `<dt>${k}</dt><dd>${v}</dd>`).join('');
}

function render(animate = false) {
  const ply = viewPly ?? game.moves.length;
  const state = positionAt(ply);
  const live = viewPly === null;
  const dests = live && !game.end && !thinking && state.turn === game.color ? G.dests(state.legal) : new Map();
  const orientation = flipped ? other(game.color) : game.color;
  G.show(board, { state, lastMove: ply > 0 ? game.moves[ply - 1] : null, dests, orientation, animate });
  renderPlayers(state);
  renderMoves(ply);
  renderStatus();
  $('#takeback').disabled = !game.moves.length;
  $('#resign').disabled = !!game.end || !game.moves.length;
  $('#prev').disabled = ply === 0;
  $('#next').disabled = live;
}

function renderMoves(ply) {
  const list = $('#moves');
  list.replaceChildren();
  list.classList.toggle('single', !G.pairs);
  const step = G.pairs ? 2 : 1;
  for (let i = 0; i < game.sans.length; i += step) {
    const row = document.createElement('div');
    row.className = 'row';
    row.innerHTML = `<span class="num">${G.pairs ? i / 2 + 1 : i + 1}.</span>`;
    for (const j of G.pairs ? [i, i + 1] : [i]) {
      const cell = document.createElement('button');
      cell.className = 'mv';
      if (j < game.sans.length) {
        cell.textContent = game.sans[j];
        if (j + 1 === ply) cell.classList.add('current');
        cell.onclick = () => goTo(j + 1);
      }
      row.append(cell);
    }
    list.append(row);
  }
  const current = list.querySelector('.current');
  if (current) current.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  else list.scrollTop = 0;
}

function renderStatus() {
  const over = $('#gameover');
  if (!game.end || viewPly !== null) {
    over.hidden = true;
    return;
  }
  const w = game.end.winner;
  over.querySelector('.result-code').textContent = G.resultCode(w);
  over.querySelector('.headline').textContent = !w ? 'Draw' : w === game.color ? 'You win' : 'Kishi wins';
  over.querySelector('.reason').textContent = game.end.reason === 'resign' ? 'You resigned' : (G.reasons[game.end.reason] ?? '');
  over.hidden = false;
}

function goTo(ply) {
  viewPly = ply >= game.moves.length ? null : Math.max(0, ply);
  render(true);
}

// ---- dialogs and controls ---------------------------------------------------

function openNewGame() {
  const dialog = $('#newgame');
  const levels = dialog.querySelector('.levels');
  levels.replaceChildren();
  for (let n = 1; n <= 8; n++) {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'lvl' + (n === game.level ? ' on' : '');
    b.innerHTML = `<b>${n}</b><span>${LEVEL_NAMES[n]}</span>`;
    b.onclick = () => {
      levels.querySelectorAll('.lvl').forEach((x) => x.classList.remove('on'));
      b.classList.add('on');
    };
    b.dataset.level = n;
    levels.append(b);
  }
  dialog.querySelector('h2').textContent = `New ${G.name.toLowerCase()} game`;
  const [first, second_] = [G.first, second()];
  dialog.querySelector('[value="first"] span').textContent = G.sideLabels.first;
  dialog.querySelector('[value="second"] span').textContent = G.sideLabels.second;
  // Chess shows its king images; shogi draws its kings, and they sit side by side under Random.
  const king = (i, side, extra = '') => {
    i.className = G.icon ? `piece koma-piece ${extra}` : `piece ${extra} ${G.kingClass[side]}`;
    i.innerHTML = G.icon ? G.icon(side) : '';
  };
  king(dialog.querySelector('[value="first"] i'), first);
  king(dialog.querySelector('[value="second"] i'), second_);
  dialog.querySelector('.halves').classList.toggle('pair', !!G.icon);
  const halves = dialog.querySelectorAll('.halves i');
  king(halves[0], first, 'half');
  king(halves[1], second_, 'half');
  dialog.showModal();
}

function wire() {
  $('#new').onclick = openNewGame;
  $('#rematch').onclick = () => newGame(game.color === G.first ? 'second' : 'first', game.level);
  $('#again').onclick = openNewGame;
  $('#takeback').onclick = takeback;
  // Game actions live in the menu: close it, then act.
  const fromMenu = (action) => () => {
    $('#settings-dialog').close();
    action();
  };
  const flip = () => {
    flipped = !flipped;
    render();
  };
  $('#flip').onclick = fromMenu(flip);
  $('#resign').onclick = fromMenu(resign);
  $('#share').onclick = fromMenu(share);
  document.querySelectorAll('[data-switch]').forEach((b) => {
    b.onclick = fromMenu(() => switchGame(b.dataset.switch));
    const mod = GAMES[b.dataset.switch];
    if (mod.icon) b.querySelector('.glyph').innerHTML = mod.icon(mod.first, 'N');
  });
  $('#prev').onclick = () => goTo((viewPly ?? game.moves.length) - 1);
  $('#next').onclick = () => goTo((viewPly ?? game.moves.length) + 1);
  $('#first').onclick = () => goTo(0);
  $('#last').onclick = () => goTo(game.moves.length);
  $('#settings').onclick = () => {
    $('#opt-eval').checked = settings.showEval;
    $('#opt-sound').checked = settings.sound;
    document.querySelectorAll('input[name="theme"]').forEach((r) => (r.checked = r.value === settings.theme));
    document.querySelectorAll('[data-switch]').forEach((b) => b.classList.toggle('on', b.dataset.switch === G.id));
    $('#share span').textContent = G.exportLabel;
    renderEngineInfo();
    $('#settings-dialog').showModal();
  };
  document.querySelectorAll('input[name="theme"]').forEach((r) => {
    r.onchange = () => (document.documentElement.dataset.board = r.value);
  });
  $('#settings-dialog').addEventListener('close', () => {
    const theme = document.querySelector('input[name="theme"]:checked')?.value ?? 'stone';
    settings = { ...settings, showEval: $('#opt-eval').checked, sound: $('#opt-sound').checked, theme };
    document.documentElement.dataset.board = theme;
    save(SETTINGS_KEY, settings);
    render();
  });
  $('#newgame').addEventListener('close', (e) => {
    const dialog = e.target;
    const choice = dialog.returnValue;
    if (!['first', 'second', 'random'].includes(choice)) return;
    const level = Number(dialog.querySelector('.lvl.on')?.dataset.level ?? game.level);
    newGame(choice, level);
  });
  document.addEventListener('keydown', (e) => {
    if (document.querySelector('dialog[open]')) return;
    if (e.key === 'ArrowLeft') $('#prev').click();
    else if (e.key === 'ArrowRight') $('#next').click();
    else if (e.key === 'ArrowUp') goTo(0);
    else if (e.key === 'ArrowDown') goTo(game.moves.length);
    else if (e.key === 'f') flip();
  });
}

/**
 * Where the host cannot send cross-origin isolation headers, the service worker
 * adds them; the first visit reloads once so the page comes back isolated.
 */
async function isolate() {
  if (!('serviceWorker' in navigator) || !self.isSecureContext) return true;
  await navigator.serviceWorker.register('sw.js').catch(() => {});
  if (self.crossOriginIsolated) return true;
  let tried = false;
  try {
    tried = sessionStorage.getItem('kishi.isolate') === '1';
    sessionStorage.setItem('kishi.isolate', '1');
  } catch {}
  if (tried) return true; // the browser will not isolate: play single-threaded
  await navigator.serviceWorker.ready;
  location.reload();
  return false;
}

async function main() {
  if (!(await isolate())) return;
  rules = await loadEngine(new URL(G.wasm, import.meta.url));
  setupBoard();
  game = loadGame();
  // A saved game whose moves no longer replay starts over.
  if (positionAt(game.moves.length).error) game = freshGame();
  startWorker();
  wire();
  render();
  if (enginesTurn(liveState())) think();
  document.body.classList.add('ready');
}

main();
