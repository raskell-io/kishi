// Go: the board, passing, the count at the end, and the record as SGF.
// The shell (app.js) does the rest.
import { GoBoard, stoneSvg } from '../go-board.js';

const SGF_LETTERS = 'abcdefghijklmnopqrs';
const COLUMNS = 'ABCDEFGHJKLMNOPQRST';

export default {
  id: 'go',
  name: 'Go',
  wasm: 'go.wasm',
  threaded: null,
  first: 'b',
  sides: { b: 'black', w: 'white' },
  sideLabels: { first: 'Black', second: 'White' },
  pairs: false,
  canPass: true,
  exportLabel: 'Copy SGF',
  reasons: { repetition: 'Repetition' },

  /** A stone for dialogs; the menu tile ('N') shows a black and a white one together. */
  icon(side, kind) {
    return kind === 'N' ? `<span class="stone-pair">${stoneSvg('w')}${stoneSvg('b')}</span>` : stoneSvg(side);
  },

  createBoard(el, onMove) {
    return new GoBoard(el, onMove);
  },

  show(board, { state, dests, orientation, animate, end, live }) {
    board.set({
      picture: state.board,
      last: state.last,
      turn: state.turn,
      dests,
      orientation,
      animate,
      territory: live && end?.territory ? end.territory : null,
    });
  },

  /** Every legal point, under one origin: a stone has nowhere to come from. */
  dests(legal) {
    return new Map([['', new Set(legal.filter((m) => m !== 'pass'))]]);
  },

  choose(board, legal, from, to) {
    return Promise.resolve(legal.includes(to) ? to : null);
  },

  /** The right side of a player's row: stones captured, and komi for white. */
  sideDetail(state, color) {
    const n = state.captures?.[color] ?? 0;
    const parts = [];
    if (n) parts.push(`${n} captured`);
    if (color === 'w') parts.push(`komi ${state.komi}`);
    return `<div class="captured go-note">${parts.join(' · ')}</div>`;
  },

  /** Kishi's estimate, shown in its row when the evaluation is on. */
  evalText(info) {
    return `${Math.round(info.winrate * 100)}% to win`;
  },

  isCapture: () => false,

  /** After two passes: count the board, taking off the stones that cannot live. */
  finish(rules) {
    const s = rules.command('score');
    const margin = Math.abs(s.margin);
    return {
      reason: 'score',
      winner: s.winner,
      detail: `${s.winner === 'b' ? 'Black' : 'White'} wins by ${margin} points`,
      code: `${s.winner === 'b' ? 'B' : 'W'}+${margin}`,
      territory: { owner: s.owner, dead: s.dead },
    };
  },

  resultCode: (winner, end) => end?.code ?? (winner === 'b' ? 'B+R' : winner === 'w' ? 'W+R' : 'draw'),

  /** The game as SGF, which every Go program and server reads. */
  export(game) {
    const kishi = `Kishi level ${game.level}`;
    const sgf = (m) => {
      if (m === 'pass') return '';
      const x = COLUMNS.indexOf(m[0].toUpperCase());
      const y = 19 - Number(m.slice(1));
      return SGF_LETTERS[x] + SGF_LETTERS[y];
    };
    const result = game.end?.code ?? (game.end?.winner ? `${game.end.winner === 'b' ? 'B' : 'W'}+R` : '');
    const head = `(;GM[1]FF[4]CA[UTF-8]SZ[19]KM[7.5]RU[Chinese]PB[${game.color === 'b' ? 'You' : kishi}]PW[${game.color === 'b' ? kishi : 'You'}]${result ? `RE[${result}]` : ''}`;
    const body = game.moves.map((m, i) => `;${i % 2 === 0 ? 'B' : 'W'}[${sgf(m)}]`).join('');
    return `${head}${body})\n`;
  },
};
