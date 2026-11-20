import * as NodeReadline from "node:readline";
import { pairWithEnvironment, readProjects } from "../src/native/projects.ts";

let session;
const reply = (value) => process.stdout.write(JSON.stringify(value) + "\n");
for await (const input of NodeReadline.createInterface({ input: process.stdin, terminal: false })) {
  try {
    const request = JSON.parse(input);
    if (request.type === "disconnect") {
      session = undefined;
      reply({ type: "disconnected" });
      continue;
    }
    if (request.type === "pair") {
      session = await pairWithEnvironment(request.link);
      reply({ type: "connected", label: session.label });
    } else if (request.type !== "refresh" || !session) {
      throw new Error("Invalid request");
    }
    const projects = await readProjects(session);
    reply({ type: "projects", projects });
  } catch {
    reply({
      type: "error",
      message: session
        ? "Couldn’t load projects. Check the connection and try again."
        : "Couldn’t pair with this environment. Check the pairing link and try again.",
    });
  }
}
