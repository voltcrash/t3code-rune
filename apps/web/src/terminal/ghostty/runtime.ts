import { GhosttyRuntime } from "@t3tools/client-runtime/terminal/ghostty/runtime";
import ghosttyWasmUrl from "./vendor/ghostty-vt.wasm?url";
import ghosttyWritePtyWasmUrl from "./vendor/ghostty-write-pty.wasm?url&no-inline";

async function readWasm(url: string, name: string) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`Unable to load ${name} (${response.status})`);
  return response.arrayBuffer();
}

let runtimePromise: Promise<GhosttyRuntime> | null = null;
export function loadGhosttyRuntime(): Promise<GhosttyRuntime> {
  runtimePromise ??= Promise.all([
    readWasm(ghosttyWasmUrl, "libghostty-vt"),
    readWasm(ghosttyWritePtyWasmUrl, "the libghostty-vt PTY trampoline"),
  ])
    .then(([wasm, trampoline]) =>
      GhosttyRuntime.load(wasm, trampoline, (message) => console.debug("[libghostty-vt]", message)),
    )
    .catch((error) => {
      runtimePromise = null;
      throw error;
    });
  return runtimePromise;
}
