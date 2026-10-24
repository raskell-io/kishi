// Searches off the main thread so the board stays responsive while Kishi thinks.
// The page names the engine first ({ type: 'init', wasm, threaded }); the threaded
// build runs with helper workers where the page allows shared memory.
import { loadEngine, loadThreadedEngine, positionCommand } from './engine.js';

const helpers = Math.max(0, Math.min(3, (navigator.hardwareConcurrency || 1) - 1));
let ready = null;

self.onmessage = async ({ data }) => {
  if (data.type === 'init') {
    const url = (file) => new URL(file, import.meta.url);
    const threaded = data.threaded ? loadThreadedEngine(helpers, url(data.threaded)) : Promise.resolve(null);
    ready = threaded.then((e) => e ?? loadEngine(url(data.wasm)));
    ready.then((engine) => self.postMessage({ type: 'ready', threads: engine.threads }));
    return;
  }
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
