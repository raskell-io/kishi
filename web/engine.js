// The wasm engine behind a one-function text interface (see engine/src/web.rs).
//
// kishi.wasm is single-threaded and runs anywhere: the page uses it for the
// rules, and the engine worker falls back to it. kishi-mt.wasm shares its
// memory with helper workers that search alongside (Lazy SMP); it needs a
// cross-origin isolated page for SharedArrayBuffer.

const imports = (memory) => ({ env: { kishi_now: () => performance.now(), ...(memory ? { memory } : {}) } });

function wrap(e) {
  e.kishi_seed(Math.random() * 2 ** 52);
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  return {
    command(text) {
      const bytes = encoder.encode(text);
      // Ask for the buffer first: it may grow memory and detach older views.
      const at = e.kishi_input(bytes.length);
      new Uint8Array(e.memory.buffer, at, bytes.length).set(bytes);
      const len = e.kishi_command(bytes.length);
      // Copy out: TextDecoder refuses views of shared memory.
      return JSON.parse(decoder.decode(new Uint8Array(e.memory.buffer, e.kishi_output(), len).slice()));
    },
  };
}

export async function loadEngine(url = new URL('kishi.wasm', import.meta.url)) {
  const { instance } = await WebAssembly.instantiate(await (await fetch(url)).arrayBuffer(), imports());
  return { ...wrap(instance.exports), threads: 1 };
}

const HELPER_STACK = 1 << 20;

/** The threaded engine with `helpers` extra workers, or null where it cannot run. */
export async function loadThreadedEngine(helpers, url = new URL('kishi-mt.wasm', import.meta.url)) {
  if (!self.crossOriginIsolated || typeof SharedArrayBuffer === 'undefined' || helpers < 1) return null;
  try {
    const module = await WebAssembly.compile(await (await fetch(url)).arrayBuffer());
    const memory = new WebAssembly.Memory({ initial: 128, maximum: 4096, shared: true });
    const instance = await WebAssembly.instantiate(module, imports(memory));
    const e = instance.exports;
    const started = [];
    for (let i = 0; i < helpers; i++) {
      const stack = e.kishi_alloc(HELPER_STACK, 16);
      const tls = e.kishi_alloc(e.__tls_size.value, e.__tls_align.value);
      const worker = new Worker(new URL('helper.js', import.meta.url));
      started.push(
        new Promise((resolve, reject) => {
          worker.onmessage = () => resolve(worker);
          worker.onerror = reject;
        }),
      );
      worker.postMessage({ module, memory, stackTop: stack + HELPER_STACK, tls });
    }
    await Promise.all(started);
    return { ...wrap({ ...e, memory }), threads: helpers + 1 };
  } catch (err) {
    console.warn('threaded engine unavailable', err);
    return null;
  }
}

export function positionCommand(moves) {
  return moves.length ? `position startpos moves ${moves.join(' ')}` : 'position startpos';
}
