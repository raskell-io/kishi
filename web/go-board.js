// The Go board: 19x19 lines on the stone board, stones drawn like Kishi's shogi
// pieces (a face in a bold ink line on an edge for depth), and input that suits
// small points: with a mouse a click places a stone; by touch, the first tap
// shows where it would go and a second tap on the same point places it.

const N = 19;
const COLUMNS = 'ABCDEFGHJKLMNOPQRST';
const CELL = 100 / N;

export function pointName(x, y) {
  return `${COLUMNS[x]}${N - y}`;
}

function pointXY(name) {
  return [COLUMNS.indexOf(name[0].toUpperCase()), N - Number(name.slice(1))];
}

/** A stone as SVG: an ink or ivory face in a bold ink line, on an edge underneath. */
export function stoneSvg(color, extra = '') {
  return `<svg class="stone ${color === 'b' ? 'black' : 'white'} ${extra}" viewBox="0 0 100 106" aria-hidden="true">` +
    '<circle class="edge" cx="50" cy="56" r="44"/><circle class="face" cx="50" cy="49" r="44"/></svg>';
}

function linesSvg() {
  const at = (i) => i + 0.5;
  let s = '<svg class="grid" viewBox="0 0 19 19" preserveAspectRatio="none" aria-hidden="true">';
  for (let i = 0; i < N; i++) {
    s += `<line x1="${at(0)}" y1="${at(i)}" x2="${at(N - 1)}" y2="${at(i)}"/><line x1="${at(i)}" y1="${at(0)}" x2="${at(i)}" y2="${at(N - 1)}"/>`;
  }
  for (const x of [3, 9, 15]) for (const y of [3, 9, 15]) s += `<circle cx="${at(x)}" cy="${at(y)}" r="0.1"/>`;
  return s + '</svg>';
}

export class GoBoard {
  constructor(el, onMove) {
    this.el = el;
    this.onMove = onMove;
    this.orientation = 'b';
    this.picture = '.'.repeat(N * N);
    this.dests = new Map();
    this.movable = false;
    this.preview = null;
    this.turn = 'b';
    el.classList.add('board', 'go');
    el.innerHTML = linesSvg();
    this.layers = {};
    for (const name of ['coords', 'marks', 'stones', 'hints']) {
      const layer = document.createElement('div');
      layer.className = name;
      el.append(layer);
      this.layers[name] = layer;
    }
    el.addEventListener('pointerdown', (e) => this.pointerDown(e));
    el.addEventListener('pointermove', (e) => this.pointerMove(e));
    el.addEventListener('pointerleave', () => this.showGhost(null));
    el.addEventListener('contextmenu', (e) => e.preventDefault());
  }

  destroy() {}

  /** Where a point sits, in units of one point, from the top-left as the player sees it. */
  place(x, y) {
    return this.orientation === 'b' ? [x * 100, y * 100] : [(N - 1 - x) * 100, (N - 1 - y) * 100];
  }

  pointAt(cx, cy) {
    const r = this.el.getBoundingClientRect();
    let x = Math.floor(((cx - r.left) / r.width) * N);
    let y = Math.floor(((cy - r.top) / r.height) * N);
    if (x < 0 || x >= N || y < 0 || y >= N) return null;
    if (this.orientation !== 'b') [x, y] = [N - 1 - x, N - 1 - y];
    return pointName(x, y);
  }

  set(s) {
    const previous = this.picture;
    this.picture = s.picture;
    this.last = s.last || null;
    this.dests = s.dests ?? new Map();
    this.movable = this.dests.size > 0;
    this.turn = s.turn;
    this.territory = s.territory ?? null;
    if (s.orientation) this.orientation = s.orientation;
    if (this.preview && !this.legal(this.preview)) this.preview = null;
    this.renderStones(s.animate ? previous : null);
    this.renderMarks();
    this.renderCoords();
    this.showGhost(this.preview);
  }

  legal(name) {
    return this.movable && !!this.dests.get('')?.has(name);
  }

  renderStones(previous) {
    const layer = this.layers.stones;
    layer.replaceChildren();
    const dead = this.territory?.dead;
    for (let i = 0; i < N * N; i++) {
      const c = this.picture[i];
      if (c === '.') continue;
      const [x, y] = [i % N, Math.floor(i / N)];
      const el = document.createElement('div');
      el.className = 'point';
      if (dead && dead[i] === '1') el.classList.add('dead');
      if (previous && previous[i] === '.') el.classList.add('placed');
      el.innerHTML = stoneSvg(c === 'x' ? 'b' : 'w');
      const [px, py] = this.place(x, y);
      el.style.transform = `translate(${px}%, ${py}%)`;
      layer.append(el);
    }
  }

  renderMarks() {
    const layer = this.layers.marks;
    layer.replaceChildren();
    const mark = (x, y, cls) => {
      const el = document.createElement('div');
      el.className = `point ${cls}`;
      const [px, py] = this.place(x, y);
      el.style.transform = `translate(${px}%, ${py}%)`;
      layer.append(el);
    };
    if (this.last && this.last !== 'pass') {
      const [x, y] = pointXY(this.last);
      mark(x, y, `last ${this.picture[y * N + x] === 'x' ? 'on-black' : 'on-white'}`);
    }
    // After the count: who owns each empty point, and the dead stones' points.
    if (this.territory) {
      for (let i = 0; i < N * N; i++) {
        const o = this.territory.owner[i];
        const empty = this.picture[i] === '.' || this.territory.dead[i] === '1';
        if (o !== '.' && empty) mark(i % N, Math.floor(i / N), `area ${o === 'b' ? 'black' : 'white'}`);
      }
    }
  }

  renderCoords() {
    const layer = this.layers.coords;
    if (layer.dataset.orientation === this.orientation) return;
    layer.dataset.orientation = this.orientation;
    layer.replaceChildren();
    for (let i = 0; i < N; i++) {
      const file = document.createElement('span');
      file.className = 'coord file';
      file.textContent = this.orientation === 'b' ? COLUMNS[i] : COLUMNS[N - 1 - i];
      file.style.left = `${i * CELL}%`;
      layer.append(file);
      const rank = document.createElement('span');
      rank.className = 'coord rank';
      rank.textContent = this.orientation === 'b' ? N - i : i + 1;
      rank.style.top = `${i * CELL}%`;
      layer.append(rank);
    }
  }

  showGhost(name) {
    const layer = this.layers.hints;
    layer.replaceChildren();
    if (!name || !this.legal(name)) return;
    const [x, y] = pointXY(name);
    const [px, py] = this.place(x, y);
    const el = document.createElement('div');
    el.className = 'point ghost';
    el.innerHTML = stoneSvg(this.turn);
    el.style.transform = `translate(${px}%, ${py}%)`;
    layer.append(el);
    if (this.preview === name) {
      // A crosshair helps find the point under a finger.
      for (const cls of ['row', 'col']) {
        const line = document.createElement('div');
        line.className = `cross ${cls}`;
        if (cls === 'row') line.style.top = `${(py / 100) * CELL + CELL / 2}%`;
        else line.style.left = `${(px / 100) * CELL + CELL / 2}%`;
        layer.append(line);
      }
    }
  }

  pointerMove(e) {
    if (e.pointerType !== 'mouse' || !this.movable) return;
    this.showGhost(this.pointAt(e.clientX, e.clientY));
  }

  pointerDown(e) {
    if (e.button !== undefined && e.button !== 0) return;
    const name = this.pointAt(e.clientX, e.clientY);
    e.preventDefault();
    this.onInteract?.();
    if (!name || !this.legal(name)) {
      this.preview = null;
      this.showGhost(null);
      return;
    }
    if (e.pointerType === 'mouse' || this.preview === name) {
      this.preview = null;
      this.showGhost(null);
      this.onMove('', name);
      return;
    }
    this.preview = name;
    this.showGhost(name);
  }
}
