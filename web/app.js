// The game: you against Kishi at one of eight levels.
import { Board, fenPieces } from './board.js';
import { loadEngine, positionCommand } from './engine.js';

const LEVEL_NAMES = ['', 'Novice', 'Beginner', 'Casual', 'Intermediate', 'Club', 'Strong', 'Expert', 'Maximum'];
const PIECE_VALUE = { P: 1, N: 3, B: 3, R: 5, Q: 9, K: 0 };
const SAVE_KEY = 'kishi.game.v1';
const SETTINGS_KEY = 'kishi.settings.v1';

const $ = (sel) => document.querySelector(sel);

let rules; // the engine on the main thread, for legal moves and notation
let worker = null;
let token = 0;
let thinking = false;
let sentAt = 0;
let engineThreads = 1;
let game = loadSaved() ?? { moves: [], sans: [], color: 'w', level: 4, end: null };
let settings = { showEval: false, sound: true, ...load(SETTINGS_KEY) };
let viewPly = null; // the ply on display while browsing history; null follows the game
let lastInfo = null;
let flipped = false;

const board = new Board($('#board'), onBoardMove);
board.onInteract = unlockAudio;

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

function loadSaved() {
  const g = load(SAVE_KEY);
  return g && Array.isArray(g.moves) ? g : null;
}

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
  worker.onmessage = ({ data }) => {
    if (data.type === 'ready') engineThreads = data.threads;
    if (data.type !== 'bestmove' || data.token !== token) return;
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

function positionAt(ply) {
  return rules.command(positionCommand(game.moves.slice(0, ply)));
}

function liveState() {
  return positionAt(game.moves.length);
}

function enginesTurn(state) {
  return !game.end && state.turn !== game.color;
}

function onBoardMove(from, to) {
  if (viewPly !== null) return;
  const state = liveState();
  const options = state.legal.filter((m) => m.startsWith(from + to));
  if (options.length === 1) play(options[0]);
  else if (options.length > 1) choosePromotion(from, to, state.turn);
}

function choosePromotion(from, to, color) {
  const picker = $('#promotion');
  picker.replaceChildren();
  const file = to.charCodeAt(0) - 97;
  const column = board.orientation === 'w' ? file : 7 - file;
  const fromTop = (color === 'w') === (board.orientation === 'w');
  ['q', 'n', 'r', 'b'].forEach((p, i) => {
    const b = document.createElement('button');
    b.className = `piece ${color}${p.toUpperCase()}`;
    b.style.left = `${column * 12.5}%`;
    b.style.top = fromTop ? `${i * 12.5}%` : `${(7 - i) * 12.5}%`;
    b.ariaLabel = { q: 'Queen', n: 'Knight', r: 'Rook', b: 'Bishop' }[p];
    b.onclick = (e) => {
      e.stopPropagation();
      picker.hidden = true;
      play(from + to + p);
    };
    picker.append(b);
  });
  picker.onclick = () => {
    picker.hidden = true;
    render();
  };
  picker.hidden = false;
}

function play(uci) {
  rules.command(positionCommand(game.moves));
  const { san } = rules.command(`san ${uci}`);
  if (!san) return;
  game.moves.push(uci);
  game.sans.push(san);
  const state = liveState();
  if (state.result) game.end = { reason: state.result, winner: state.result === 'checkmate' ? (state.turn === 'w' ? 'b' : 'w') : null };
  save(SAVE_KEY, game);
  click(san.includes('x'));
  viewPly = null;
  render(true);
  if (enginesTurn(state)) think();
}

function newGame(color, level) {
  stopThinking();
  worker.postMessage({ type: 'newgame' });
  if (color === 'random') color = Math.random() < 0.5 ? 'w' : 'b';
  game = { moves: [], sans: [], color, level, end: null };
  flipped = false;
  lastInfo = null;
  viewPly = null;
  save(SAVE_KEY, game);
  render();
  if (enginesTurn(liveState())) think();
}

function takeback() {
  if (!game.moves.length) return;
  stopThinking();
  // Undo Kishi's reply, if any, and your last move, so it is your turn again.
  while (game.moves.length) {
    const ply = game.moves.length - 1;
    const yours = (ply % 2 === 0) === (game.color === 'w');
    game.moves.pop();
    game.sans.pop();
    if (yours) break;
  }
  game.end = null;
  lastInfo = null;
  viewPly = null;
  save(SAVE_KEY, game);
  render();
  // Back at the start as black: Kishi opens again.
  if (enginesTurn(liveState())) think();
}

function resign() {
  if (game.end || !game.moves.length) return;
  stopThinking();
  game.end = { reason: 'resign', winner: game.color === 'w' ? 'b' : 'w' };
  save(SAVE_KEY, game);
  render();
}

function pgn() {
  const date = new Date().toISOString().slice(0, 10).replaceAll('-', '.');
  const you = 'You';
  const kishi = `Kishi level ${game.level}`;
  const result = !game.end ? '*' : game.end.winner === 'w' ? '1-0' : game.end.winner === 'b' ? '0-1' : '1/2-1/2';
  const tags = [
    ['Event', 'Casual game'],
    ['Site', location.host || 'Kishi'],
    ['Date', date],
    ['White', game.color === 'w' ? you : kishi],
    ['Black', game.color === 'w' ? kishi : you],
    ['Result', result],
  ];
  let body = '';
  game.sans.forEach((san, i) => {
    if (i % 2 === 0) body += `${i / 2 + 1}. `;
    body += san + ' ';
  });
  return tags.map(([k, v]) => `[${k} "${v}"]`).join('\n') + '\n\n' + body + result + '\n';
}

async function share() {
  const text = pgn();
  try {
    if (navigator.share) await navigator.share({ title: 'Kishi game', text });
    else {
      await navigator.clipboard.writeText(text);
      toast('PGN copied');
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

// ---- rendering --------------------------------------------------------------

function material(fen) {
  const counts = { w: {}, b: {} };
  for (const p of Object.values(fenPieces(fen))) counts[p[0]][p[1]] = (counts[p[0]][p[1]] ?? 0) + 1;
  const start = { P: 8, N: 2, B: 2, R: 2, Q: 1 };
  const out = { w: [], b: [], diff: 0 };
  for (const k of ['Q', 'R', 'B', 'N', 'P']) {
    // Pieces each side has captured from the other.
    const takenFromBlack = Math.max(0, start[k] - (counts.b[k] ?? 0));
    const takenFromWhite = Math.max(0, start[k] - (counts.w[k] ?? 0));
    for (let i = 0; i < takenFromBlack; i++) out.w.push('b' + k);
    for (let i = 0; i < takenFromWhite; i++) out.b.push('w' + k);
    out.diff += ((counts.w[k] ?? 0) - (counts.b[k] ?? 0)) * PIECE_VALUE[k];
  }
  return out;
}

function formatScore(info) {
  if (!info) return '';
  // Scores are from Kishi's side; show them from white's.
  const sign = game.color === 'w' ? -1 : 1;
  if (info.mate !== null) return `#${info.mate * sign}`;
  const s = (info.score * sign) / 100;
  return (s > 0 ? '+' : '') + s.toFixed(1);
}

function renderPlayer(el, color, isEngine, mat) {
  const name = isEngine ? `Kishi <span class="level">level ${game.level} · ${LEVEL_NAMES[game.level]}</span>` : 'You';
  const captured = mat[color].map((p) => `<i class="mini ${p}"></i>`).join('');
  const lead = color === 'w' ? mat.diff : -mat.diff;
  let extra = '';
  if (isEngine && thinking) extra = '<span class="thinking"><b></b><b></b><b></b></span>';
  else if (isEngine && settings.showEval && lastInfo) {
    extra = `<span class="eval">${lastInfo.book ? 'book' : `${formatScore(lastInfo)} · d${lastInfo.depth}`}</span>`;
  }
  el.innerHTML = `
    <div class="who"><span class="dot ${color}"></span><span class="name">${name}</span>${extra}</div>
    <div class="captured">${captured}${lead > 0 ? `<span class="lead">+${lead}</span>` : ''}</div>`;
}

function render(animate = false) {
  const ply = viewPly ?? game.moves.length;
  const state = positionAt(ply);
  const live = viewPly === null;
  const dests = new Map();
  if (live && !game.end && !thinking && state.turn === game.color) {
    for (const m of state.legal) {
      const from = m.slice(0, 2);
      if (!dests.has(from)) dests.set(from, new Set());
      dests.get(from).add(m.slice(2, 4));
    }
  }
  const last = ply > 0 ? game.moves[ply - 1] : null;
  const orientation = flipped ? (game.color === 'w' ? 'b' : 'w') : game.color;
  board.set({
    fen: state.fen,
    lastMove: last ? [last.slice(0, 2), last.slice(2, 4)] : null,
    check: state.check,
    dests,
    movable: dests.size > 0,
    orientation,
    animate,
  });

  const mat = material(state.fen);
  const top = orientation === 'w' ? 'b' : 'w';
  renderPlayer($('#top'), top, top !== game.color, mat);
  renderPlayer($('#bottom'), orientation, orientation !== game.color, mat);
  $('#top').classList.toggle('to-move', live && !game.end && state.turn === top);
  $('#bottom').classList.toggle('to-move', live && !game.end && state.turn !== top);

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
  for (let i = 0; i < game.sans.length; i += 2) {
    const row = document.createElement('div');
    row.className = 'row';
    row.innerHTML = `<span class="num">${i / 2 + 1}.</span>`;
    for (const j of [i, i + 1]) {
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
  const reasons = {
    checkmate: 'Checkmate',
    stalemate: 'Stalemate',
    fifty: 'Fifty-move rule',
    material: 'Insufficient material',
    repetition: 'Threefold repetition',
    resign: 'You resigned',
  };
  const w = game.end.winner;
  const headline = !w ? 'Draw' : w === game.color ? 'You win' : 'Kishi wins';
  over.querySelector('.headline').textContent = headline;
  over.querySelector('.reason').textContent = reasons[game.end.reason] ?? '';
  over.hidden = false;
}

function goTo(ply) {
  if (ply >= game.moves.length) viewPly = null;
  else viewPly = Math.max(0, ply);
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
  dialog.showModal();
}

function wire() {
  $('#new').onclick = openNewGame;
  $('#rematch').onclick = () => newGame(game.color === 'w' ? 'b' : 'w', game.level);
  $('#again').onclick = openNewGame;
  $('#takeback').onclick = takeback;
  $('#flip').onclick = () => {
    flipped = !flipped;
    render();
  };
  $('#resign').onclick = resign;
  $('#share').onclick = share;
  $('#prev').onclick = () => goTo((viewPly ?? game.moves.length) - 1);
  $('#next').onclick = () => goTo((viewPly ?? game.moves.length) + 1);
  $('#first').onclick = () => goTo(0);
  $('#last').onclick = () => goTo(game.moves.length);
  $('#settings').onclick = () => {
    $('#opt-eval').checked = settings.showEval;
    $('#opt-sound').checked = settings.sound;
    $('#engine-info').textContent = `Searching on ${engineThreads} thread${engineThreads > 1 ? 's' : ''} of this device.`;
    $('#settings-dialog').showModal();
  };
  $('#settings-dialog').addEventListener('close', () => {
    settings = { showEval: $('#opt-eval').checked, sound: $('#opt-sound').checked };
    save(SETTINGS_KEY, settings);
    render();
  });
  $('#newgame').addEventListener('close', (e) => {
    const dialog = e.target;
    const color = dialog.returnValue;
    if (!['w', 'b', 'random'].includes(color)) return;
    const level = Number(dialog.querySelector('.lvl.on')?.dataset.level ?? game.level);
    newGame(color, level);
  });
  document.addEventListener('keydown', (e) => {
    if (document.querySelector('dialog[open]')) return;
    if (e.key === 'ArrowLeft') $('#prev').click();
    else if (e.key === 'ArrowRight') $('#next').click();
    else if (e.key === 'ArrowUp') goTo(0);
    else if (e.key === 'ArrowDown') goTo(game.moves.length);
    else if (e.key === 'f') $('#flip').click();
  });
}

async function main() {
  rules = await loadEngine();
  startWorker();
  wire();
  // A saved game whose moves no longer replay starts over.
  if (positionAt(game.moves.length).error) game = { moves: [], sans: [], color: 'w', level: 4, end: null };
  render();
  if (enginesTurn(liveState())) think();
  if ('serviceWorker' in navigator && location.protocol === 'https:') {
    navigator.serviceWorker.register('sw.js').catch(() => {});
  }
  document.body.classList.add('ready');
}

main();
