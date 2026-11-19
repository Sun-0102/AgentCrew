// The Windows restricted-token runner must inherit a console from its host.
// Keep the SDK's JSON-RPC pipes intact and hide only a console we allocate.
if (process.platform === "win32") {
  const { createRequire } = require("node:module");
  const hostRequire = createRequire(process.argv[1]);
  const nativeRequire = createRequire(hostRequire.resolve("@deepseek-ai/dsh-win32-process"));
  const koffi = nativeRequire("koffi");
  const kernel = koffi.load("kernel32.dll");
  const user = koffi.load("user32.dll");
  const getConsole = kernel.func("void * __stdcall GetConsoleWindow()");
  if (!getConsole()) {
    const allocConsole = kernel.func("int __stdcall AllocConsole()");
    const getStd = kernel.func("void * __stdcall GetStdHandle(uint32)");
    const setStd = kernel.func("int __stdcall SetStdHandle(uint32, void *)");
    const lastError = kernel.func("uint32 __stdcall GetLastError()");
    const showWindow = user.func("int __stdcall ShowWindow(void *, int)");
    const isVisible = user.func("int __stdcall IsWindowVisible(void *)");
    const ids = [0xfffffff6, 0xfffffff5, 0xfffffff4];
    const handles = ids.map(id => getStd(id));
    if (!allocConsole()) throw new Error(`Harness AllocConsole failed (${lastError()})`);
    try {
      const window = getConsole();
      if (!window) throw new Error("Harness console was not attached");
      showWindow(window, 0);
      if (isVisible(window)) throw new Error("Harness console could not be hidden");
    } finally {
      ids.forEach((id, index) => {
        if (!setStd(id, handles[index])) throw new Error(`Harness stdio restoration failed (${lastError()})`);
      });
    }
  }
}
