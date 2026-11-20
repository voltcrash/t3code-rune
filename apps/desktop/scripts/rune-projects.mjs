import desktopPackage from "../package.json" with { type: "json" };
import {
  isNightlyDesktopVersion,
  resolveDesktopAppStageLabel,
} from "../src/updates/updateChannels.ts";
import * as Effect from "effect/Effect";
import { respondToApproval, respondToQuestion } from "../src/native/runtimeRequests.ts";
import { followWindow, readWindow, windowReply } from "../src/native/conversation.ts";
import { readLaunchOptions, launchThread, NewThreadUnavailable } from "../src/native/newThread.ts";
import { sendThreadMessage, stopThreadRun } from "../src/native/messages.ts";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import * as NodeReadline from "node:readline";
import { isLocalServerStartupError, startLocalServer } from "../src/native/localServer.ts";
import {
  HomePathNeedsBrowseAccess,
  changeProject,
  pairWithEnvironment,
  readWorkspace,
} from "../src/native/projects.ts";

const packaged = process.argv[2] === "--packaged";
const buildStage = resolveDesktopAppStageLabel({
  isDevelopment: !packaged,
  appVersion: desktopPackage.version,
}).toLowerCase();

let session;
let localSession;
let server;
let watching;
const pendingLaunches = new Map();
const lifetime = new AbortController();
const input = NodeReadline.createInterface({ input: process.stdin, terminal: false });
const write = (value) => {
  if (!lifetime.signal.aborted) process.stdout.write(JSON.stringify(value) + "\n");
};
const close = () => {
  stopWatching();
  pendingLaunches.clear();
  lifetime.abort();
  void server?.stop();
};
input.once("close", close);
process.stdout.once("error", () => {
  close();
  input.close();
});

function stopWatching() {
  const prior = watching;
  watching = undefined;
  prior?.controller.abort();
  return prior;
}

function watch(owner, thread, id, state) {
  const controller = new AbortController();
  const record = { owner, thread, id, state, controller, synchronized: false };
  watching = record;
  const signal = AbortSignal.any([owner.signal, controller.signal]);
  void followWindow(owner, state, signal, (update) => {
    if (watching !== record || signal.aborted) return;
    if (update.kind === "status") record.synchronized = update.state === "live";
    write({ type: "live", watch: id, thread, sequence: state.sequence, update });
    if (update.kind === "deleted") stopWatching();
  });
}

async function openThread(owner, request, reply) {
  const prior = stopWatching();
  const same = prior?.owner === owner && prior.thread === request.thread ? prior : undefined;
  try {
    const state = await readWindow(owner, request, same?.state);
    if (session !== owner || lifetime.signal.aborted) return;
    if (state.projection.thread.deletedAt !== null || state.projection.thread.archivedAt !== null)
      return reply({ type: "threadunavailable", thread: request.thread });
    if (!request.cursor && state === same?.state && request.sequence === state.sequence) {
      reply({ type: "unchanged" });
    } else {
      reply({ type: "thread", thread: request.thread, ...windowReply(state) });
    }
    watch(owner, request.thread, request.id, state);
  } catch (error) {
    if (error?._tag === "EnvironmentResourceNotFoundError" && error.reason === "thread_not_found") {
      if (session === owner) reply({ type: "threadunavailable", thread: request.thread });
      return;
    }
    if (same && session === owner && !lifetime.signal.aborted)
      watch(owner, same.thread, same.id, same.state);
    throw error;
  }
}

async function interruptRun(owner, request, reply) {
  const result = {
    type: "stopresult",
    thread: request.thread,
    run: request.run,
    operation: request.operation,
  };
  if (!owner?.canOperate)
    return reply({ ...result, error: "This connection does not have permission to stop the run." });
  if (
    watching?.owner !== owner ||
    watching.thread !== request.thread ||
    watching.state.activeRun !== request.run
  )
    return reply({
      ...result,
      error: "The running turn changed. Refresh the thread to check its status.",
    });
  try {
    await Effect.runPromise(
      stopThreadRun(owner, request).pipe(
        Effect.scoped,
        Effect.timeout("10 seconds"),
        Effect.provide(owner.http),
      ),
      { signal: owner.signal },
    );
    if (session === owner) reply(result);
  } catch {
    if (session === owner)
      reply({ ...result, error: "Couldn’t confirm the interruption. Check the run and retry." });
  }
}

function localServer() {
  if (!server) {
    const cwd = NodePath.resolve(import.meta.dirname, packaged ? "server" : "../../server");
    const started = startLocalServer({
      cwd,
      entryPath: NodePath.join(cwd, packaged ? "dist/bin.mjs" : "src/bin.ts"),
      t3Home: packaged
        ? NodePath.resolve(
            process.env.T3CODE_HOME?.trim() || NodePath.join(NodeOS.homedir(), ".t3", "rune"),
          )
        : NodePath.resolve(import.meta.dirname, "../../../.t3/rune"),
    });
    server = started;
    void started.stopped.then(() => {
      if (server !== started) return;
      server = undefined;
      if (!localSession) return;
      stopWatching();
      pendingLaunches.clear();
      session = localSession = undefined;
      write({ type: "localclosed" });
    });
  }
  return server;
}

function requestError(error, type, editing) {
  if (error instanceof NewThreadUnavailable) return error.message;
  if (type === "preparescratch")
    return "Couldn’t open the scratch workspace on this environment. Try again.";
  if (type === "preparethread") return "Couldn’t load models for this project. Try again.";
  if (type === "launchthread" && error?._tag === "OrchestrationV2ThreadLaunchError")
    return error.message;
  if (type === "launchthread")
    return "Couldn’t confirm that the thread started. Retry to check delivery.";
  if (error instanceof HomePathNeedsBrowseAccess) {
    return "Enter the full folder path. This connection cannot look up the environment’s home folder.";
  }
  if (type === "sendmessage") {
    if (error?._tag === "EnvironmentAuthorizationError")
      return "This connection does not have permission to send messages.";
    if (error?._tag === "OrchestrationV2DispatchCommandError" && error.detail) return error.detail;
    return "Couldn’t confirm that the message was sent. Retry to check delivery.";
  }
  if (editing)
    return type === "addproject"
      ? "Couldn’t add the project. Check that the folder exists on this environment and that you have permission."
      : "Couldn’t update the project. Refresh and try again.";
  if (session)
    return type === "thread"
      ? "Couldn’t load this conversation. Try again."
      : "Couldn’t load this workspace. Check the connection and try again.";
  if (type === "local")
    return isLocalServerStartupError(error)
      ? error.message
      : "Couldn’t start the local environment. Try again or paste a pairing link.";
  return "Couldn’t pair with this environment. Check the pairing link and try again.";
}

async function handleWorkspaceRequest(request, reply) {
  if (lifetime.signal.aborted) return;
  let reading;
  let editing = false;
  try {
    if (request.type === "disconnect") {
      stopWatching();
      pendingLaunches.clear();
      session = localSession = undefined;
      reply({ type: "disconnected" });
      return;
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
      stopWatching();
      pendingLaunches.clear();
      reply({
        type: "connected",
        label: session.label,
        local: Boolean(localSession),
        can_operate: session.canOperate,
        stage: isNightlyDesktopVersion(session.serverVersion) ? "nightly" : buildStage,
      });
    } else if (
      request.type === "preparescratch" ||
      request.type === "preparethread" ||
      request.type === "launchthread"
    ) {
      reading = session;
      if (!reading?.canOperate) throw new Error("Read-only session");
      if (request.type !== "launchthread") {
        const options = await Effect.runPromise(
          readLaunchOptions(
            reading,
            request.type === "preparescratch" ? null : request.project,
          ).pipe(Effect.timeout("20 seconds"), Effect.scoped, Effect.provide(reading.http)),
          { signal: reading.signal },
        );
        if (reading === session) reply({ type: "launchoptions", ...options });
      } else {
        const thread = await Effect.runPromise(
          launchThread(reading, request, pendingLaunches).pipe(
            Effect.timeout("60 seconds"),
            Effect.scoped,
            Effect.provide(reading.http),
          ),
          { signal: reading.signal },
        );
        if (reading === session) reply({ type: "threadlaunched", thread });
      }
      return;
    } else if (request.type === "thread" && session) {
      reading = session;
      await openThread(reading, request, reply);
      return;
    } else if (request.type === "approval" || request.type === "question") {
      reading = session;
      const result = { type: "requestresult", thread: request.thread, version: request.version };
      const current = watching;
      if (
        !reading?.canOperate ||
        !current?.synchronized ||
        current.owner !== reading ||
        current.thread !== request.thread
      ) {
        reply({
          ...result,
          error:
            "This request changed or is unavailable on this connection. Refresh the conversation.",
        });
        return;
      }
      const approval =
        request.type === "approval" &&
        current.state.requests.approvals.find(
          (a) =>
            a.id === request.request &&
            a.version === request.version &&
            a.live &&
            a.options.some((c) => c.decision === request.decision),
        );
      const question =
        request.type === "question" &&
        current.state.requests.questions.find(
          (q) => q.requestId === request.request && q.version === request.version,
        );
      const command = approval
        ? respondToApproval(reading, request).pipe(Effect.as(true))
        : question
          ? respondToQuestion(reading, question, request)
          : Effect.succeed(false);
      try {
        const accepted = await Effect.runPromise(
          command.pipe(Effect.scoped, Effect.timeout("15 seconds"), Effect.provide(reading.http)),
          { signal: reading.signal },
        );
        if (reading === session)
          reply(
            accepted
              ? result
              : {
                  ...result,
                  error:
                    "This response is unavailable. Refresh the conversation and check the request.",
                },
          );
      } catch {
        if (reading === session)
          reply({
            ...result,
            error:
              "Couldn’t confirm the response. Refresh to check the request before trying again.",
          });
      }
      return;
    } else if (request.type === "sendmessage" && session) {
      reading = session;
      if (!reading.canOperate) throw new Error("Read-only session");
      await Effect.runPromise(
        sendThreadMessage(reading, request).pipe(
          Effect.scoped,
          Effect.timeout("60 seconds"),
          Effect.provide(reading.http),
        ),
        { signal: reading.signal },
      );
      if (session === reading)
        reply({ type: "messagesent", thread: request.thread, operation: request.operation });
      return;
    } else if ((request.type === "addproject" || request.type === "archiveproject") && session) {
      editing = true;
      reading = session;
      if (!reading.canOperate) throw new Error("Read-only session");
      await changeProject(reading, request);
      if (session !== reading) return;
      reply({ type: "projectsaved", added: request.type === "addproject" });
      editing = false;
    } else if (request.type !== "refresh" || !session) {
      throw new Error("Invalid request");
    }
    reading = session;
    const workspace = await readWorkspace(reading);
    if (session === reading) {
      if (watching && !workspace.threads.some((thread) => thread.id === watching.thread))
        stopWatching();
      reply({ type: "workspace", ...workspace });
    }
  } catch (error) {
    if (reading && session !== reading) return;
    const message = requestError(error, request?.type, editing);
    if (request?.type === "launchthread")
      reply({
        type: "launchfailed",
        project: request.project,
        uncertain: pendingLaunches.has(request.operation),
        message,
      });
    else reply({ type: "error", message });
  }
}

try {
  let workspaceRequests = Promise.resolve();
  for await (const line of input) {
    if (lifetime.signal.aborted) break;
    let request;
    try {
      request = JSON.parse(line);
    } catch {
      write({ type: "error", message: "Invalid desktop request." });
      continue;
    }
    const reply = (value) => write({ id: request?.id, ...value });
    if (request?.type === "stoprun") {
      void interruptRun(session, request, reply);
    } else {
      workspaceRequests = workspaceRequests.then(() => handleWorkspaceRequest(request, reply));
    }
  }
  await workspaceRequests;
} finally {
  close();
  await server?.stopped;
}
