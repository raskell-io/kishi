// A helper thread for the threaded engine: shares the engine's memory, gets
// its own stack and thread-local storage, then searches whenever the engine does.
self.onmessage = ({ data: { module, memory, stackTop, tls } }) => {
  const instance = new WebAssembly.Instance(module, {
    env: { memory, kishi_now: () => performance.now() },
  });
  const e = instance.exports;
  e.__stack_pointer.value = stackTop;
  e.__wasm_init_tls(tls);
  self.postMessage('ready');
  e.kishi_helper_run(); // never returns
};
