import * as NodeChildProcess from "node:child_process";
import * as NodeURL from "node:url";

const desktop = NodeChildProcess.spawnSync(
  "cargo",
  ["run", "--locked", "--bin", "t3-code", "--", ...process.argv.slice(2)],
  {
    cwd: NodeURL.fileURLToPath(new URL("../native/", import.meta.url)),
    env: {
      ...process.env,
      T3CODE_RUNE_NODE: process.execPath,
      T3CODE_RUNE_WORKER: NodeURL.fileURLToPath(new URL("./rune-projects.mjs", import.meta.url)),
    },
    stdio: "inherit",
  },
);
if (desktop.error) console.error(desktop.error.message);
process.exitCode = desktop.status ?? 1;
