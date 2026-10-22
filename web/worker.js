// Searches off the main thread so the board stays responsive while Kishi thinks.
// Uses the threaded engine with helper workers where the page allows it.
import { loadEngine, loadThreadedEngine, positionCommand } from './engine.js';

const helpers = Math.max(0, Math.min(3, (navigator.hardwareConcurrency || 1) - 1));
const ready = loadThreadedEngine(helpers).then((e) => e ?? loadEngine());

ready.then((engine) => self.postMessage({ type: 'ready', threads: engine.threads }));

self.onmessage = async ({ data }) => {
  const engine = await ready;
  if (data.type === 'newgame') {
    engine.command('newgame');
    return;
  }
  if (data.type === 'search') {
    engine.command(positionCommand(data.moves));
    const go = data.ms ? `go ms ${data.ms}` : `go level ${data.level}`;
    self.postMessage({ type: 'bestmove', token: data.token, ...engine.command(go) });
  }
};
