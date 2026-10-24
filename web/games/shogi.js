// Shogi: the board, the hands in the player rows, drops and the promotion
// choice, and the game record for export. The shell (app.js) does the rest.
import { ShogiBoard, parseSfen, kanji } from '../shogi-board.js';

const HAND_ORDER = ['R', 'B', 'G', 'S', 'N', 'L', 'P'];
const START_SFEN = 'lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1';

export default {
  id: 'shogi',
  name: 'Shogi',
  wasm: 'shogi.wasm',
  threaded: null,
  first: 'b',
  sides: { b: 'sente', w: 'gote' },
  sideLabels: { first: 'Sente', second: 'Gote' },
  kingClass: { b: 'koma-icon sente', w: 'koma-icon gote' },
  pairs: false,
  exportLabel: 'Copy record',
  reasons: {
    checkmate: 'Checkmate (tsumi)',
    repetition: 'Fourfold repetition (sennichite)',
    moves: 'Move limit',
  },

  createBoard(el, onMove) {
    return new ShogiBoard(el, onMove);
  },

  show(board, { state, lastMove, dests, orientation, animate }) {
    let last = null;
    if (lastMove) last = lastMove[1] === '*' ? [null, lastMove.slice(2, 4)] : [lastMove.slice(0, 2), lastMove.slice(2, 4)];
    board.set({ sfen: state.sfen, lastMove: last, check: state.check, dests, movable: dests.size > 0, orientation, animate });
  },

  /** Legal moves grouped by origin: a square, or "P*" for a pawn in hand. */
  dests(legal) {
    const dests = new Map();
    for (const m of legal) {
      const from = m[1] === '*' ? m.slice(0, 2) : m.slice(0, 2);
      if (!dests.has(from)) dests.set(from, new Set());
      dests.get(from).add(m.slice(2, 4));
    }
    return dests;
  },

  /** The move for a tap from `from` to `to`; asks whether to promote when both are legal. */
  choose(board, legal, from, to) {
    const options = legal.filter((m) => m.slice(0, 4) === from + to);
    if (options.length <= 1) return Promise.resolve(options[0] ?? null);
    const piece = board.pieces[from];
    return new Promise((resolve) => {
      const picker = document.querySelector('#promotion');
      picker.replaceChildren();
      picker.classList.add('shogi');
      const [x, y] = board.place(to);
      const box = document.createElement('div');
      box.className = 'promote-choice';
      box.style.left = `${(x / 100) * 11.111}%`;
      box.style.top = `${Math.min(7, y / 100) * 11.111}%`;
      for (const [promote, kind] of [[true, '+' + piece.kind], [false, piece.kind]]) {
        const b = document.createElement('button');
        b.className = `koma${promote ? ' promoted' : ''}`;
        b.textContent = kanji(kind, piece.side);
        b.ariaLabel = promote ? 'Promote' : 'Do not promote';
        b.onclick = (e) => {
          e.stopPropagation();
          picker.hidden = true;
          picker.classList.remove('shogi');
          resolve(from + to + (promote ? '+' : ''));
        };
        box.append(b);
      }
      picker.append(box);
      picker.onclick = () => {
        picker.hidden = true;
        picker.classList.remove('shogi');
        resolve(null);
      };
      picker.hidden = false;
    });
  },

  /** The right side of a player's row: the pieces in their hand, which are tappable when it is theirs to drop. */
  sideDetail(state, color) {
    const { hands } = parseSfen(state.sfen);
    const pieces = HAND_ORDER.filter((k) => hands[color][k]).map(
      (k) => `<button class="hand-piece" data-kind="${k}" aria-label="${k} in hand"><span class="koma">${kanji(k, color)}</span>${hands[color][k] > 1 ? `<sub>${hands[color][k]}</sub>` : ''}</button>`,
    );
    return `<div class="hand">${pieces.join('')}</div>`;
  },

  /** Connects a player's hand to the board, so tapping a piece picks it up for a drop. */
  wireSide(el, board, isYours) {
    el.querySelectorAll('.hand-piece').forEach((b) => {
      const key = `${b.dataset.kind}*`;
      b.disabled = !isYours || !board.dests.has(key);
      b.classList.toggle('on', board.selected === key);
      b.onclick = () => board.selectDrop(b.dataset.kind);
    });
  },

  isCapture: (san) => san.includes('x'),

  resultCode: (winner) => (!winner ? 'draw' : winner === 'b' ? 'sente wins' : 'gote wins'),

  /** The game as a numbered move list, plus the USI line any engine or lishogi can load. */
  export(game) {
    const kishi = `Kishi level ${game.level}`;
    const lines = [
      `Sente: ${game.color === 'b' ? 'You' : kishi}`,
      `Gote: ${game.color === 'b' ? kishi : 'You'}`,
      '',
      game.sans.map((s, i) => `${i + 1}. ${s}`).join('\n'),
      '',
      `position sfen ${START_SFEN} moves ${game.moves.join(' ')}`,
    ];
    return lines.join('\n') + '\n';
  },
};
