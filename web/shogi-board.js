// The shogi board: nine by nine squares in the same colours as the chess board,
// pentagon pieces lettered in kanji, and input by tap or drag. Drops start from the hands, which the game
// module draws in the player rows and hands over with selectDrop().

export const KANJI = {
  P: '歩', L: '香', N: '桂', S: '銀', G: '金', B: '角', R: '飛',
  '+P': 'と', '+L': '杏', '+N': '圭', '+S': '全', '+B': '馬', '+R': '龍',
};
const KING = { b: '玉', w: '王' };
const RANKS = 'abcdefghi';

export function kanji(kind, side) {
  return kind === 'K' ? KING[side] : KANJI[kind];
}

/** Square name (USI, e.g. "7g") of a board index: row-major from gote's back rank, 9-file first. */
export function squareName(index) {
  return `${9 - (index % 9)}${RANKS[Math.floor(index / 9)]}`;
}

function squareIndex(name) {
  return RANKS.indexOf(name[1]) * 9 + (9 - Number(name[0]));
}

/** Board pieces ({ "7g": { kind: "P", side: "b" } }), hands and turn of an SFEN. */
export function parseSfen(sfen) {
  const [placement, turn, hand] = sfen.split(' ');
  const pieces = {};
  placement.split('/').forEach((row, r) => {
    let c = 0;
    let promote = false;
    for (const ch of row) {
      if (/\d/.test(ch)) c += Number(ch);
      else if (ch === '+') promote = true;
      else {
        const side = ch === ch.toUpperCase() ? 'b' : 'w';
        pieces[squareName(r * 9 + c)] = { kind: (promote ? '+' : '') + ch.toUpperCase(), side };
        promote = false;
        c++;
      }
    }
  });
  const hands = { b: {}, w: {} };
  let count = '';
  for (const ch of hand === '-' ? '' : hand ?? '') {
    if (/\d/.test(ch)) count += ch;
    else {
      const side = ch === ch.toUpperCase() ? 'b' : 'w';
      hands[side][ch.toUpperCase()] = Number(count || 1);
      count = '';
    }
  }
  return { pieces, hands, turn };
}

export class ShogiBoard {
  constructor(el, onMove) {
    this.el = el;
    this.onMove = onMove;
    this.orientation = 'b';
    this.pieces = {};
    this.dests = new Map();
    this.movable = false;
    this.selected = null; // a square, or "P*" for a piece in hand
    this.lastMove = null;
    this.check = null;
    this.drag = null;
    this.onSelect = null;

    el.classList.add('board', 'shogi');
    this.layers = {};
    for (const name of ['coords', 'highlights', 'pieces', 'hints']) {
      const layer = document.createElement('div');
      layer.className = name;
      el.append(layer);
      this.layers[name] = layer;
    }
    el.addEventListener('pointerdown', (e) => this.pointerDown(e));
    this.moveHandler = (e) => this.pointerMove(e);
    this.upHandler = (e) => this.pointerUp(e);
    window.addEventListener('pointermove', this.moveHandler, { passive: false });
    window.addEventListener('pointerup', this.upHandler);
    el.addEventListener('contextmenu', (e) => e.preventDefault());
  }

  destroy() {
    window.removeEventListener('pointermove', this.moveHandler);
    window.removeEventListener('pointerup', this.upHandler);
  }

  /** A square's position, in units of one square, from the top-left as the player sees it. */
  place(name) {
    const i = squareIndex(name);
    const [r, c] = [Math.floor(i / 9), i % 9];
    return this.orientation === 'b' ? [c * 100, r * 100] : [(8 - c) * 100, (8 - r) * 100];
  }

  squareAt(x, y) {
    const rect = this.el.getBoundingClientRect();
    const c = Math.floor(((x - rect.left) / rect.width) * 9);
    const r = Math.floor(((y - rect.top) / rect.height) * 9);
    if (c < 0 || c > 8 || r < 0 || r > 8) return null;
    return this.orientation === 'b' ? squareName(r * 9 + c) : squareName((8 - r) * 9 + (8 - c));
  }

  set(s) {
    const previous = this.pieces;
    const parsed = parseSfen(s.sfen);
    this.pieces = parsed.pieces;
    this.hands = parsed.hands;
    this.lastMove = s.lastMove ?? null;
    this.check = s.check || null;
    this.dests = s.dests ?? new Map();
    this.movable = !!s.movable;
    if (s.orientation) this.orientation = s.orientation;
    if (this.selected && !this.dests.has(this.selected)) this.selected = null;
    this.renderPieces(s.animate ? previous : null);
    this.renderHighlights();
    this.renderCoords();
  }

  face(kind, side) {
    const el = document.createElement('span');
    el.className = `koma${kind.startsWith('+') ? ' promoted' : ''}${side !== this.orientation ? ' away' : ''}`;
    el.textContent = kanji(kind, side);
    return el;
  }

  renderPieces(previous) {
    const layer = this.layers.pieces;
    layer.replaceChildren();
    const vanished = previous ? Object.entries(previous).filter(([sq, p]) => this.pieces[sq]?.side !== p.side || this.pieces[sq]?.kind !== p.kind) : [];
    for (const [sq, p] of Object.entries(this.pieces)) {
      const el = document.createElement('div');
      el.className = 'piece';
      el.dataset.square = sq;
      el.append(this.face(p.kind, p.side));
      const [x, y] = this.place(sq);
      let from = null;
      const before = previous?.[sq];
      if (previous && (!before || before.side !== p.side || before.kind !== p.kind)) {
        // The piece that left: same side, same kind or its unpromoted form.
        const base = p.kind.replace('+', '');
        const i = vanished.findIndex(([, v]) => v.side === p.side && v.kind.replace('+', '') === base);
        if (i >= 0) from = vanished.splice(i, 1)[0][0];
      }
      if (from) {
        const [fx, fy] = this.place(from);
        el.style.transform = `translate(${fx}%, ${fy}%)`;
        requestAnimationFrame(() =>
          requestAnimationFrame(() => {
            el.classList.add('animating');
            el.style.transform = `translate(${x}%, ${y}%)`;
          }),
        );
      } else {
        el.style.transform = `translate(${x}%, ${y}%)`;
        if (previous && !before) el.classList.add('dropped');
      }
      layer.append(el);
    }
  }

  renderHighlights() {
    const layer = this.layers.highlights;
    layer.replaceChildren();
    const mark = (sq, cls, parent = layer) => {
      const el = document.createElement('div');
      el.className = `square ${cls}`;
      const [x, y] = this.place(sq);
      el.style.transform = `translate(${x}%, ${y}%)`;
      parent.append(el);
    };
    if (this.lastMove) for (const sq of this.lastMove) if (sq) mark(sq, 'last-move');
    if (this.check) mark(this.check, 'check');
    const hints = this.layers.hints;
    hints.replaceChildren();
    if (this.selected) {
      if (!this.selected.endsWith('*')) mark(this.selected, 'selected');
      for (const to of this.dests.get(this.selected) ?? []) mark(to, this.pieces[to] ? 'dest capture' : 'dest', hints);
    }
  }

  /** Files along the bottom and ranks down the left, as on the chess board, in USI terms (7f). */
  renderCoords() {
    const layer = this.layers.coords;
    if (layer.dataset.orientation === this.orientation) return;
    layer.dataset.orientation = this.orientation;
    layer.replaceChildren();
    // Squares alternate from a light top-left corner; with nine a side, so does every corner.
    const shade = (r, c) => ((r + c) % 2 === 0 ? 'on-light' : 'on-dark');
    for (let i = 0; i < 9; i++) {
      const file = document.createElement('span');
      file.className = `coord file ${shade(8, i)}`;
      file.textContent = this.orientation === 'b' ? 9 - i : i + 1;
      file.style.left = `${i * 11.1111}%`;
      layer.append(file);
      const rank = document.createElement('span');
      rank.className = `coord rank ${shade(i, 0)}`;
      rank.textContent = this.orientation === 'b' ? RANKS[i] : RANKS[8 - i];
      rank.style.top = `${i * 11.1111}%`;
      layer.append(rank);
    }
  }

  select(origin) {
    this.selected = origin;
    this.renderHighlights();
    this.onSelect?.(origin);
  }

  /** Called by the hand in the player row: pick up a piece to drop, or put it back. */
  selectDrop(kind) {
    const key = `${kind}*`;
    this.select(this.selected === key || !this.dests.has(key) ? null : key);
  }

  tryMove(from, to) {
    if (this.dests.get(from)?.has(to)) {
      this.select(null);
      this.onMove(from, to);
      return true;
    }
    return false;
  }

  pointerDown(e) {
    if (e.button !== undefined && e.button !== 0) return;
    const sq = this.squareAt(e.clientX, e.clientY);
    if (!sq) return;
    e.preventDefault();
    this.onInteract?.();
    if (this.selected && this.selected !== sq && this.tryMove(this.selected, sq)) return;
    if (this.movable && this.dests.has(sq)) {
      const wasSelected = this.selected === sq;
      this.select(sq);
      const pieceEl = this.layers.pieces.querySelector(`[data-square="${sq}"]`);
      if (pieceEl) {
        this.drag = { from: sq, el: pieceEl, startX: e.clientX, startY: e.clientY, moved: false, wasSelected };
        pieceEl.classList.add('dragging');
      }
    } else {
      this.select(null);
    }
  }

  pointerMove(e) {
    const d = this.drag;
    if (!d) return;
    e.preventDefault();
    const rect = this.el.getBoundingClientRect();
    const size = rect.width / 9;
    if (!d.moved && Math.hypot(e.clientX - d.startX, e.clientY - d.startY) < size * 0.15) return;
    d.moved = true;
    const x = e.clientX - rect.left - size / 2;
    const y = e.clientY - rect.top - size / 2;
    d.el.style.transform = `translate(${(x / size) * 100}%, ${(y / size) * 100}%) scale(1.12)`;
  }

  pointerUp(e) {
    const d = this.drag;
    if (!d) return;
    this.drag = null;
    d.el.classList.remove('dragging');
    const to = this.squareAt(e.clientX, e.clientY);
    if (d.moved && to && to !== d.from && this.tryMove(d.from, to)) return;
    const [x, y] = this.place(d.from);
    d.el.style.transform = `translate(${x}%, ${y}%)`;
    if (!d.moved && d.wasSelected) this.select(null);
  }
}
