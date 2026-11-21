import * as NodeChildProcess from "node:child_process";
import * as NodeFS from "node:fs";
import * as NodeURL from "node:url";

const watching = process.argv[2] === "--watch";
const built = process.argv[2] === "--built";
const owned = built || watching;
const extra = process.argv.slice(owned ? 3 : 2);
const under = (path) => NodeURL.fileURLToPath(new URL(path, import.meta.url));
// oxlint-disable-next-line t3code/no-global-process-runtime -- This launcher starts the built executable for its own host.
const binary = process.platform === "win32" ? "t3-code.exe" : "t3-code";
const watchers = [];
let app;
let restart = false;
let stopping = false;
let timer;

function launch() {
  if (stopping) return;
  restart = false;
  let executable = "cargo";
  let worker = NodeURL.fileURLToPath(new URL("./rune-projects.mjs", import.meta.url));
  if (built) {
    try {
      const generation = NodeFS.readFileSync(under("../dist/current"), "utf8").trim();
      if (!/^[a-f0-9]{64}$/u.test(generation)) throw new Error("Invalid desktop build");
      executable = under(`../dist/${generation}/${binary}`);
      worker = under(`../dist/${generation}/rune-projects.mjs`);
      NodeFS.accessSync(executable, NodeFS.constants.X_OK);
      NodeFS.accessSync(worker, NodeFS.constants.R_OK);
    } catch {
      console.error("Run vp run build:desktop before starting the desktop app.");
      process.exitCode = 1;
      return;
    }
  }
  app = NodeChildProcess.spawn(
    executable,
    [
      ...(built ? ["--source-build"] : ["run", "--locked", "--bin", "t3-code", "--"]),
      ...(watching ? ["--integrated-dev"] : []),
      ...extra,
    ],
    {
      cwd: under("../native/"),
      env: {
        ...process.env,
        T3CODE_RUNE_NODE: process.execPath,
        T3CODE_RUNE_WORKER: worker,
      },
      stdio: [owned ? "pipe" : "inherit", "inherit", "inherit"],
    },
  );
  app.once("error", (error) => {
    console.error(
      built && error.code === "ENOENT"
        ? "Run vp run build:desktop before starting the desktop app."
        : error.message,
    );
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
}
if (owned) {
  process.once("SIGINT", stop);
  process.once("SIGTERM", stop);
}
launch();
