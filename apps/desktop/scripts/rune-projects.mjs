import * as NodeReadline from "node:readline";
import { isLocalServerStartupError, startLocalServer } from "../src/native/localServer.ts";
import { pairWithEnvironment, readProjects } from "../src/native/projects.ts";

let session;
let localSession;
let server;
const lifetime = new AbortController();
const input = NodeReadline.createInterface({ input: process.stdin, terminal: false });
const write = (value) => {
  if (!lifetime.signal.aborted) process.stdout.write(JSON.stringify(value) + "\n");
};
const close = () => {
  lifetime.abort();
  void server?.stop();
};
input.once("close", close);
process.stdout.once("error", () => {
  close();
  input.close();
});

function localServer() {
  if (!server) {
    const started = startLocalServer();
    server = started;
    void started.stopped.then(() => {
      if (server !== started) return;
      server = undefined;
      if (!localSession) return;
      session = localSession = undefined;
      write({ type: "localclosed" });
    });
  }
  return server;
}

try {
  for await (const line of input) {
    if (lifetime.signal.aborted) break;
    let request;
    let reading;
    const reply = (value) => write({ id: request?.id, ...value });
    try {
      request = JSON.parse(line);
      if (request.type === "disconnect") {
        session = localSession = undefined;
        reply({ type: "disconnected" });
        continue;
      }
      if (request.type === "local" || request.type === "pair") {
        if (request.type === "local") {
          const host = localServer();
          const connected = await host.connect(lifetime.signal);
          if (server !== host) throw new Error("Local server stopped");
          session = localSession = connected;
        } else {
          session = await pairWithEnvironment(request.link, lifetime.signal);
          localSession = undefined;
        }
        reply({ type: "connected", label: session.label });
      } else if (request.type !== "refresh" || !session) {
        throw new Error("Invalid request");
      }
      reading = session;
      const projects = await readProjects(reading);
      if (session === reading) reply({ type: "projects", projects });
    } catch (error) {
      if (reading && session !== reading) continue;
      reply({
        type: "error",
        message: session
          ? "Couldn’t load projects. Check the connection and try again."
          : request?.type === "local"
            ? isLocalServerStartupError(error)
              ? error.message
              : "Couldn’t start the local environment. Try again or paste a pairing link."
            : "Couldn’t pair with this environment. Check the pairing link and try again.",
      });
    }
  }
} finally {
  close();
  await server?.stopped;
}
