// Chess: the board, how a tap becomes a move, notation for export, and what
// the player rows show. The shell (app.js) does the rest.
import { Board, fenPieces } from '../board.js';

const PIECE_VALUE = { P: 1, N: 3, B: 3, R: 5, Q: 9, K: 0 };

/** Pieces each side has taken, and the material difference, from a FEN. */
function material(fen) {
  const counts = { w: {}, b: {} };
  for (const p of Object.values(fenPieces(fen))) counts[p[0]][p[1]] = (counts[p[0]][p[1]] ?? 0) + 1;
  const start = { P: 8, N: 2, B: 2, R: 2, Q: 1 };
  const out = { w: [], b: [], diff: 0 };
  for (const k of ['Q', 'R', 'B', 'N', 'P']) {
    const takenFromBlack = Math.max(0, start[k] - (counts.b[k] ?? 0));
    const takenFromWhite = Math.max(0, start[k] - (counts.w[k] ?? 0));
    for (let i = 0; i < takenFromBlack; i++) out.w.push('b' + k);
    for (let i = 0; i < takenFromWhite; i++) out.b.push('w' + k);
    out.diff += ((counts.w[k] ?? 0) - (counts.b[k] ?? 0)) * PIECE_VALUE[k];
  }
  return out;
}

export default {
  id: 'chess',
  name: 'Chess',
  wasm: 'kishi.wasm',
  threaded: 'kishi-mt.wasm',
  first: 'w',
  sides: { w: 'white', b: 'black' },
  sideLabels: { first: 'White', second: 'Black' },
  kingClass: { w: 'wK', b: 'bK' },
  pairs: true,
  exportLabel: 'Copy PGN',
  reasons: {
    checkmate: 'Checkmate',
    stalemate: 'Stalemate',
    fifty: 'Fifty-move rule',
    material: 'Insufficient material',
    repetition: 'Threefold repetition',
  },

  createBoard(el, onMove) {
    return new Board(el, onMove);
  },

  show(board, { state, lastMove, dests, orientation, animate }) {
    board.set({
      fen: state.fen,
      lastMove: lastMove ? [lastMove.slice(0, 2), lastMove.slice(2, 4)] : null,
      check: state.check,
      dests,
      movable: dests.size > 0,
      orientation,
      animate,
    });
  },

  /** Legal moves grouped by origin square: from -> Set of destinations. */
  dests(legal) {
    const dests = new Map();
    for (const m of legal) {
      const from = m.slice(0, 2);
      if (!dests.has(from)) dests.set(from, new Set());
      dests.get(from).add(m.slice(2, 4));
    }
    return dests;
  },

  /** The move for a tap from `from` to `to`, asking which piece to promote to when it matters. */
  choose(board, legal, from, to, turn) {
    const options = legal.filter((m) => m.startsWith(from + to));
    if (options.length <= 1) return Promise.resolve(options[0] ?? null);
    return new Promise((resolve) => {
      const picker = document.querySelector('#promotion');
      picker.replaceChildren();
      const file = to.charCodeAt(0) - 97;
      const column = board.orientation === 'w' ? file : 7 - file;
      const fromTop = (turn === 'w') === (board.orientation === 'w');
      ['q', 'n', 'r', 'b'].forEach((p, i) => {
        const b = document.createElement('button');
        b.className = `piece ${turn}${p.toUpperCase()}`;
        b.style.left = `${column * 12.5}%`;
        b.style.top = fromTop ? `${i * 12.5}%` : `${(7 - i) * 12.5}%`;
        b.ariaLabel = { q: 'Queen', n: 'Knight', r: 'Rook', b: 'Bishop' }[p];
        b.onclick = (e) => {
          e.stopPropagation();
          picker.hidden = true;
          resolve(from + to + p);
        };
        picker.append(b);
      });
      picker.onclick = () => {
        picker.hidden = true;
        resolve(null);
      };
      picker.hidden = false;
    });
  },

  /** The right side of a player's row: what they have captured, and their lead. */
  sideDetail(state, color) {
    const mat = material(state.fen);
    const lead = color === 'w' ? mat.diff : -mat.diff;
    return `<div class="captured">${mat[color].map((p) => `<i class="mini ${p}"></i>`).join('')}${lead > 0 ? `<span class="lead">+${lead}</span>` : ''}</div>`;
  },

  isCapture: (san) => san.includes('x'),

  resultCode: (winner) => (!winner ? '½-½' : winner === 'w' ? '1-0' : '0-1'),

  /** The game as PGN. */
  export(game) {
    const date = new Date().toISOString().slice(0, 10).replaceAll('-', '.');
    const kishi = `Kishi level ${game.level}`;
    const w = game.end?.winner;
    const result = !game.end ? '*' : w === 'w' ? '1-0' : w === 'b' ? '0-1' : '1/2-1/2';
    const tags = [
      ['Event', 'Casual game'],
      ['Site', location.host || 'Kishi'],
      ['Date', date],
      ['White', game.color === 'w' ? 'You' : kishi],
      ['Black', game.color === 'w' ? kishi : 'You'],
      ['Result', result],
    ];
    let body = '';
    game.sans.forEach((san, i) => {
      if (i % 2 === 0) body += `${i / 2 + 1}. `;
      body += san + ' ';
    });
    return tags.map(([k, v]) => `[${k} "${v}"]`).join('\n') + '\n\n' + body + result + '\n';
  },
};
