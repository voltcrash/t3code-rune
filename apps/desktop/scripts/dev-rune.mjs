import * as NodeChildProcess from "node:child_process";
import * as NodeFS from "node:fs";
import * as NodeURL from "node:url";

const watching = process.argv[2] === "--watch";
const extra = process.argv.slice(watching ? 3 : 2);
const under = (path) => NodeURL.fileURLToPath(new URL(path, import.meta.url));
const watchers = [];
let app;
let restart = false;
let stopping = false;
let timer;

function launch() {
  if (stopping) return;
  restart = false;
  app = NodeChildProcess.spawn(
    "cargo",
    [
      "run",
      "--locked",
      "--bin",
      "t3-code",
      "--",
      ...(watching ? ["--integrated-dev"] : []),
      ...extra,
    ],
    {
      cwd: under("../native/"),
      env: {
        ...process.env,
        T3CODE_RUNE_NODE: process.execPath,
        T3CODE_RUNE_WORKER: NodeURL.fileURLToPath(new URL("./rune-projects.mjs", import.meta.url)),
      },
      stdio: [watching ? "pipe" : "inherit", "inherit", "inherit"],
    },
  );
  app.once("error", (error) => {
    console.error(error.message);
    process.exitCode = 1;
    stop();
  });
  app.once("close", (code) => {
    app = undefined;
    if (restart && !stopping) return launch();
    if (watching && !stopping && code !== 0) return;
    process.exitCode ??= code ?? 1;
    stop();
  });
}
function stop() {
  stopping = true;
  clearTimeout(timer);
  for (const watcher of watchers) watcher.close();
  app?.stdin?.end();
}
function changed() {
  clearTimeout(timer);
  timer = setTimeout(() => {
    if (stopping) return;
    if (app) {
      restart = true;
      app.stdin.end();
    } else launch();
  }, 150);
}
if (watching) {
  for (const dir of ["../native/src/", "../src/native/", "../src/backend/"]) {
    watchers.push(NodeFS.watch(under(dir), { recursive: true }, changed));
  }
  for (const [dir, names] of [
    ["../native/", ["Cargo.toml", "Cargo.lock"]],
    ["./", ["rune-projects.mjs"]],
  ]) {
    watchers.push(
      NodeFS.watch(under(dir), (_event, file) => {
        if (file === null || names.includes(file)) changed();
      }),
    );
  }
  process.once("SIGINT", stop);
  process.once("SIGTERM", stop);
}
launch();
