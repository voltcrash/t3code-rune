import { CommandId, ORCHESTRATION_V2_WS_METHODS, ThreadId } from "@t3tools/contracts";
import { trackVisibleThread, visitFromWindow } from "../src/native/visits.ts";
import { watchWorkspace } from "../src/native/workspaceFeed.ts";
import { openTerminal } from "./rune-terminal.mjs";
import { readThreadModels, setThreadModel, ModelUnavailable } from "../src/native/models.ts";
import { PermissionUnavailable, readThreadPermissions } from "../src/native/permissions.ts";
import { openEnvironmentRpc } from "../src/native/environmentRpc.ts";
import { AttachmentUnavailable } from "../src/native/attachments.ts";
import { readImage } from "../src/native/images.ts";
import * as Semaphore from "effect/Semaphore";
import * as NodeTimersPromises from "node:timers/promises";
import { turnItemNeedsDetailFetch } from "@t3tools/client-runtime/work-log/item-detail";
import desktopPackage from "../package.json" with { type: "json" };
import {
  isNightlyDesktopVersion,
  resolveDesktopAppStageLabel,
} from "../src/updates/updateChannels.ts";
import * as Effect from "effect/Effect";
import { respondToApproval, respondToQuestion } from "../src/native/runtimeRequests.ts";
import { readToolDetail } from "../src/native/toolDetail.ts";
import { readFileDiff } from "../src/native/fileChange.ts";
import { readCommandOutput } from "../src/native/commandOutput.ts";
import { followWindow, readWindow, windowReply } from "../src/native/conversation.ts";
import { readLaunchOptions, launchThread, NewThreadUnavailable } from "../src/native/newThread.ts";
import { sendThreadMessage, stopThreadRun } from "../src/native/messages.ts";
import * as NodeFS from "node:fs";
import * as NodeCrypto from "node:crypto";
import * as NodeSqlite from "node:sqlite";
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
const sourceBuild = process.argv[2] === "--source-build";
const buildStage = resolveDesktopAppStageLabel({
  isDevelopment: !packaged && !sourceBuild,
  appVersion: import.meta.t3codeVersion ?? desktopPackage.version,
}).toLowerCase();

let session;
let localSession;
let server;
let watching;
const pendingLaunches = new Map();
const imageReads = Semaphore.makeUnsafe(3);
const sentImages = new Map();
const lifetime = new AbortController();
const input = NodeReadline.createInterface({ input: process.stdin, terminal: false });
const write = (value) => {
  if (!lifetime.signal.aborted) process.stdout.write(JSON.stringify(value) + "\n");
};
const close = () => {
  leaveThread();
  session?.legacyVisits?.close();
  stopWorkspaceFeed();
  stopWatching();
  pendingLaunches.clear();
  lifetime.abort();
  for (const sent of sentImages.values()) sent.handled.resolve();
  void server?.stop();
};
input.once("close", close);
process.stdout.once("error", () => {
  close();
  input.close();
});

let activeVisit;
let visibleThread;
function updateVisibleVisit() {
  const id = visibleThread;
  if (!id || activeVisit || watching?.thread !== id || watching.owner !== session) return;
  activeVisit = trackVisibleThread(session, id, (at) => workspaceFeed?.seen(id, at));
  workspaceFeed?.opened(id, Date.now());
  activeVisit.update(
    workspaceFeed?.visitSource(id) ?? visitFromWindow(session, watching.state.projection),
  );
}
function leaveThread(id = visibleThread) {
  if (id !== visibleThread) return;
  visibleThread = undefined;
  activeVisit?.close();
  activeVisit = undefined;
}
let workspaceFeed;
function stopWorkspaceFeed() {
  workspaceFeed?.controller.abort();
  workspaceFeed = undefined;
}

function startWorkspaceFeed(owner, initial, watch) {
  stopWorkspaceFeed();
  const controller = new AbortController();
  const signal = AbortSignal.any([owner.signal, controller.signal]);
  const feed = watchWorkspace(
    owner,
    initial,
    signal,
    (changes) => {
      if (watching?.owner === owner && changes.removed_threads.includes(watching.thread)) {
        leaveThread(watching.thread);
        stopWatching();
      }
      write({ type: "workspacelive", watch, ...changes });
    },
    () => write({ type: "workspacepaused", watch }),
    (offset) => write({ type: "workspaceclock", watch, offset }),
    () => activeVisit,
  );
  workspaceFeed = { watch, controller, ...feed };
  if (activeVisit) activeVisit.update(feed.visitSource(activeVisit.id));
}

function stopWatching() {
  const prior = watching;
  watching = undefined;
  prior?.controller.abort();
  return prior;
}

function watch(owner, thread, id, state) {
  const controller = new AbortController();
  const record = {
    owner,
    thread,
    id,
    state,
    controller,
    tools: new Map(),
    images: new Map(),
    synchronized: false,
  };
  watching = record;
  updateVisibleVisit();
  const signal = AbortSignal.any([owner.signal, controller.signal]);
  void followWindow(
    owner,
    state,
    signal,
    (update) => {
      if (watching !== record || signal.aborted) return;
      if (update.kind === "status") {
        record.synchronized = update.state === "live";
        if (!record.synchronized) record.needsDetailSync = true;
        else if (record.needsDetailSync) {
          for (const tool of record.tools.values()) refreshDetail(record, tool, "resync");
          record.needsDetailSync = false;
        }
      }
      write({ type: "live", watch: id, thread, sequence: state.sequence, update });
      if (update.kind === "deleted") {
        leaveThread(thread);
        stopWatching();
      }
      if (update.kind === "window" || update.kind === "splice") {
        if (record.file) refreshDetail(record, record.file);
        for (const tool of record.tools.values())
          refreshDetail(record, tool, update.kind === "window" ? "resync" : undefined);
        if (update.kind === "window") record.needsDetailSync = false;
      }
      const output = record.output;
      if (output && (update.kind === "splice" || update.kind === "window")) {
        const row = state.rows[state.indices.get(output.request.key)];
        if (row?.role !== "command") {
          output.controller.abort();
          record.output = undefined;
        } else if (!owner.streamsCommandOutput && update.kind === "window") {
          output.reload();
        }
      }
    },
    (threadId, itemId) => {
      if (record.tools.size) {
        const tool = record.tools.get(JSON.stringify([threadId, itemId]));
        if (tool) refreshDetail(record, tool, "changed");
      }
      const output = record.output;
      if (!owner.streamsCommandOutput && output?.threadId === threadId && output.itemId === itemId)
        output.reload();
    },
  );
}

async function inspectCommand(request) {
  const record = watching;
  if (!record || record.id !== request.watch) return;
  record.output?.controller.abort();
  record.output = undefined;
  if (!request.key) return;
  const controller = new AbortController();

  const signal = AbortSignal.any([
    record.controller.signal,
    controller.signal,
    record.owner.signal,
  ]);
  const writeText = (text) => {
    if (!signal.aborted) write({ type: "commandoutput", operation: request.operation, text });
  };
  const row = record.state.rows[record.state.indices.get(request.key)];
  if (row?.role !== "command") return writeText("Command unavailable.");
  const [threadId, itemId] = JSON.parse(request.key);
  let lastText = "";
  const emit = (text) => {
    lastText = text;
    writeText(text);
  };
  let loading = false;
  let dirty = false;
  async function reload() {
    dirty = true;
    if (loading) return;
    loading = true;
    while (dirty && !signal.aborted) {
      dirty = false;
      try {
        await Effect.runPromise(
          readCommandOutput(record.owner, { threadId, itemId }, emit).pipe(
            Effect.scoped,
            Effect.provide(record.owner.http),
          ),
          { signal },
        );
      } catch {
        const error = "Couldn’t load output. Reopen the command to retry.";
        writeText(lastText ? lastText + "\n" + error : error);
      }
    }
    loading = false;
  }
  record.output = { controller, request, threadId, itemId, reload };
  await reload();
}

function refreshDetail(record, detail, updated) {
  const row = record.state.rows[record.state.indices.get(detail.key)];
  const file = detail.type === "filediff";
  const revision = (file ? row?.file : row?.tool)?.revision ?? null;
  if (detail.revision === revision && !updated) return;
  const [threadId, itemId] = JSON.parse(detail.key);
  const item =
    !file && row?.tool
      ? record.state.projection.visibleTurnItems.findLast(
          (entry) => entry.sourceItemId === itemId && entry.sourceThreadId === threadId,
        )?.item
      : undefined;
  if (detail.revision === revision && item) {
    if (updated !== "resync" && detail.source === item) return;
    if (detail.fetch && turnItemNeedsDetailFetch(item)) {
      detail.again = true;
      return;
    }
  }
  detail.again = false;
  detail.source = item;
  detail.revision = revision;
  detail.fetch?.abort();
  const fetch = new AbortController();
  detail.fetch = fetch;
  const signal = AbortSignal.any([record.owner.signal, record.controller.signal, fetch.signal]);
  const emit = (value) => {
    if (!signal.aborted)
      write({ type: detail.type, key: detail.key, operation: detail.operation, ...value });
  };
  if (!revision || (!file && !item))
    return emit({
      message: file ? "This file change is unavailable." : "This tool call is unavailable.",
    });
  const input = { threadId, itemId };
  const root = record.state.projection.thread.worktreePath ?? record.state.projectRoot;
  const load = file
    ? readFileDiff(record.owner, input, root)
    : readToolDetail(record.owner, input, item, root);
  void Effect.runPromise(
    load.pipe(Effect.scoped, Effect.timeout("20 seconds"), Effect.provide(record.owner.http)),
    { signal },
  )
    .then(emit, () =>
      emit({
        message: file
          ? "Couldn’t load the recorded diff. Reopen it to retry."
          : "Couldn’t load tool details. Reopen the tool to retry.",
      }),
    )
    .then(async () => {
      // Coalesce compact streaming updates into one trailing lookup per open tool.
      if (revision === "live" && item && turnItemNeedsDetailFetch(item) && !signal.aborted)
        await NodeTimersPromises.setTimeout(200, undefined, { signal }).catch(() => {});
      if (signal.aborted) return;
      detail.fetch = undefined;
      if (detail.again) refreshDetail(record, detail, "resync");
    });
}

async function inspectImage(request) {
  const record = watching;
  if (!record || record.id !== request.watch) return;
  record.images.get(request.operation)?.abort();
  if (!request.open) return;
  const controller = new AbortController();
  record.images.set(request.operation, controller);
  const signal = AbortSignal.any([
    record.owner.signal,
    record.controller.signal,
    controller.signal,
  ]);
  const result = {
    type: "image",
    watch: request.watch,
    key: request.key,
    operation: request.operation,
    full: request.full,
  };
  try {
    const [resource] = JSON.parse(request.key);
    if (resource._tag !== "attachment" && resource._tag !== "tool-output-image") return;
    await Effect.runPromise(
      Effect.uninterruptibleMask((restore) =>
        restore(
          readImage(record.owner, resource).pipe(Effect.scoped, Effect.timeout("20 seconds")),
        ).pipe(
          Effect.flatMap((data) => {
            if (signal.aborted) return Effect.void;
            // Bytes already sent occupy a decoder slot across watch changes until Rust acknowledges them.
            const sent = { watch: request.watch, handled: Promise.withResolvers() };
            sentImages.set(request.operation, sent);
            write({ ...result, data });
            return Effect.promise(() => sent.handled.promise).pipe(
              Effect.ensuring(Effect.sync(() => sentImages.delete(request.operation))),
            );
          }),
        ),
      ).pipe(imageReads.withPermit, Effect.provide(record.owner.http)),
      { signal },
    );
  } catch {
    if (!signal.aborted) write(result);
  } finally {
    if (record.images.get(request.operation) === controller)
      record.images.delete(request.operation);
  }
}

function inspectDetail(request) {
  const record = watching;
  if (!record || record.id !== request.watch) return;
  const file = request.type === "inspectfile";
  const previous = file ? record.file : record.tools.get(request.key);
  previous?.fetch?.abort();
  const detail =
    request.key && request.open !== false
      ? { key: request.key, operation: request.operation, type: file ? "filediff" : "tooldetail" }
      : undefined;
  if (file) record.file = detail;
  else if (detail) record.tools.set(request.key, detail);
  else record.tools.delete(request.key);
  if (detail) refreshDetail(record, detail);
}

async function openThread(owner, request, reply) {
  const prior = stopWatching();
  const same = prior?.owner === owner && prior.thread === request.thread ? prior : undefined;
  try {
    const state = await readWindow(
      owner,
      { ...request, projectRoot: request.project_root },
      same?.state,
    );
    if (session !== owner || lifetime.signal.aborted) return;
    if (state.projection.thread.deletedAt !== null || state.projection.thread.archivedAt !== null) {
      leaveThread(request.thread);
      return reply({ type: "threadunavailable", thread: request.thread });
    }
    if (!request.cursor && state === same?.state && request.sequence === state.sequence) {
      reply({ type: "unchanged" });
    } else {
      reply({ type: "thread", thread: request.thread, ...windowReply(state) });
    }
    watch(owner, request.thread, request.id, state);
  } catch (error) {
    if (error?._tag === "EnvironmentResourceNotFoundError" && error.reason === "thread_not_found") {
      if (session === owner) {
        leaveThread(request.thread);
        reply({ type: "threadunavailable", thread: request.thread });
      }
      return;
    }
    if (same && session === owner && !lifetime.signal.aborted)
      watch(owner, same.thread, same.id, same.state);
    throw error;
  }
}

async function loadComposerOptions(request, reply) {
  const record = watching;
  const target = { type: request.type, watch: request.watch, operation: request.operation };
  if (!record || record.id !== request.watch || !record.owner.canOperate)
    return reply({ ...target, instance: "", error: "Reopen this thread to load the picker." });
  record.composerRead?.abort();
  const controller = new AbortController();
  record.composerRead = controller;
  const signal = AbortSignal.any([
    record.controller.signal,
    record.owner.signal,
    controller.signal,
  ]);
  const thread = record.state.projection.thread;
  const instance =
    request.type === "threadpermissions" && request.instance
      ? request.instance
      : thread.modelSelection.instanceId;
  try {
    const options = await Effect.runPromise(
      openEnvironmentRpc(record.owner).pipe(
        Effect.flatMap((rpc) =>
          request.type === "threadmodels"
            ? readThreadModels(record.owner, rpc, record.state.projection)
            : readThreadPermissions(rpc, instance),
        ),
        Effect.scoped,
        Effect.timeout("12 seconds"),
        Effect.provide(record.owner.http),
      ),
      { signal },
    );
    if (!signal.aborted) reply({ ...target, instance, options });
  } catch (error) {
    if (!signal.aborted)
      reply({
        ...target,
        instance,
        error:
          error instanceof ModelUnavailable || error instanceof PermissionUnavailable
            ? error.message
            : "Couldn’t load these options. Reopen the picker to retry.",
      });
  } finally {
    if (record.composerRead === controller) record.composerRead = undefined;
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

const desktop = NodePath.resolve(import.meta.dirname, sourceBuild ? "../.." : "..");
const devHome = process.argv[2] === "--integrated-dev" && process.env.T3CODE_RUNE_DEV_HOME;
const t3Home = packaged
  ? NodePath.resolve(
      process.env.T3CODE_HOME?.trim() || NodePath.join(NodeOS.homedir(), ".t3", "rune"),
    )
  : devHome || NodePath.resolve(desktop, "../../.t3/rune");
function readPreference(name) {
  try {
    return NodeFS.readFileSync(NodePath.join(t3Home, name), "utf8").trim();
  } catch (error) {
    if (error.code !== "ENOENT") process.stderr.write(`Couldn’t read ${name}.\n`);
    return undefined;
  }
}

function savePreference(name, value) {
  const path = NodePath.join(t3Home, name);
  if (value === null) {
    NodeFS.rmSync(path, { force: true });
    return;
  }
  const temp = path + "." + NodeCrypto.randomUUID();
  NodeFS.mkdirSync(t3Home, { recursive: true });
  try {
    NodeFS.writeFileSync(temp, String(value) + "\n", { mode: 0o600, flag: "wx" });
    NodeFS.renameSync(temp, path);
  } finally {
    NodeFS.rmSync(temp, { force: true });
  }
}

function validAppearance(mode) {
  return mode === "system" || mode === "light" || mode === "dark";
}
const savedAppearance = readPreference("desktop-appearance");
let appearance = validAppearance(savedAppearance) ? savedAppearance : "system";
write({ type: "appearance", mode: appearance });

function setAppearance(mode) {
  try {
    if (!validAppearance(mode)) throw new Error("Unknown appearance");
    savePreference("desktop-appearance", mode);
    appearance = mode;
    write({ type: "appearance", mode });
  } catch {
    write({
      type: "appearance",
      mode: appearance,
      error: "Couldn’t save the appearance on this computer.",
    });
  }
}

const validWidth = (width) => Number.isInteger(width) && width > 0 && width <= 0xffffffff;
const savedWidth = Number(readPreference("desktop-sidebar-width"));
write({
  type: "sidebarlayout",
  width: validWidth(savedWidth) ? savedWidth : null,
  open: readPreference("desktop-sidebar-open") !== "false",
});

function setSidebarLayout(request) {
  try {
    if (request.type === "setsidebaropen" && typeof request.open === "boolean") {
      savePreference("desktop-sidebar-open", request.open);
    } else if (
      request.type === "setsidebarwidth" &&
      (request.width === null || validWidth(request.width))
    ) {
      savePreference("desktop-sidebar-width", request.width);
    }
  } catch {
    write({ type: "sidebarlayout", error: "Couldn’t save the sidebar layout on this computer." });
  }
}

function openVisitDatabase() {
  NodeFS.mkdirSync(t3Home, { recursive: true });
  return new NodeSqlite.DatabaseSync(NodePath.join(t3Home, "desktop-visits.sqlite"), {
    timeout: 1000,
  });
}
function localServer() {
  if (!server) {
    const cwd = packaged
      ? NodePath.join(import.meta.dirname, "server")
      : NodePath.resolve(desktop, "../server");
    const started = startLocalServer({
      cwd,
      entryPath: NodePath.join(cwd, packaged || sourceBuild ? "dist/bin.mjs" : "src/bin.ts"),
      t3Home,
      ...(devHome
        ? {
            dev: { port: Number(process.env.T3CODE_PORT), webUrl: process.env.VITE_DEV_SERVER_URL },
          }
        : {}),
    });
    server = started;
    void started.stopped.then(() => {
      if (server !== started) return;
      server = undefined;
      if (!localSession) return;
      leaveThread();
      session?.legacyVisits?.close();
      stopWorkspaceFeed();
      stopWatching();
      pendingLaunches.clear();
      session = localSession = undefined;
      write({ type: "localclosed" });
    });
  }
  return server;
}

function requestError(error, type, editing) {
  if (
    error instanceof ModelUnavailable ||
    error instanceof AttachmentUnavailable ||
    error instanceof NewThreadUnavailable ||
    error instanceof PermissionUnavailable
  )
    return error.message;
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
  if (type === "setthreadmodel")
    return "Couldn’t confirm the model change. Reopen the picker to check the thread.";
  if (type === "setsettlement") {
    if (error?._tag === "OrchestrationV2DispatchCommandError" && error.detail) return error.detail;
    return "Couldn’t confirm the thread change. Refresh to check its status.";
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
      leaveThread();
      session?.legacyVisits?.close();
      stopWorkspaceFeed();
      stopWatching();
      pendingLaunches.clear();
      session = localSession = undefined;
      reply({ type: "disconnected" });
      return;
    }
    if (request.type === "local" || request.type === "pair") {
      leaveThread();
      session?.legacyVisits?.close();
      if (request.type === "local") {
        const host = localServer();
        const connected = await host.connect(lifetime.signal, openVisitDatabase);
        if (server !== host) throw new Error("Local server stopped");
        session = localSession = connected;
      } else {
        session = await pairWithEnvironment(request.link, lifetime.signal, openVisitDatabase);
        localSession = undefined;
      }
      stopWorkspaceFeed();
      stopWatching();
      pendingLaunches.clear();
      reply({
        type: "connected",
        label: session.label,
        local: Boolean(localSession),
        can_operate: session.canOperate,
        terminal_read: session.canReadTerminal || session.canOperateTerminal,
        terminal_write: session.canOperateTerminal,
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
        const launch = await Effect.runPromise(
          launchThread(reading, request, pendingLaunches).pipe(
            Effect.timeout(request.files?.length ? "5 minutes" : "60 seconds"),
            Effect.scoped,
            Effect.provide(reading.http),
          ),
          { signal: reading.signal },
        );
        if (reading !== session) return;
        let thread = null;
        try {
          thread = (await workspaceFeed?.launched(launch.thread, launch.sequence)) ?? null;
        } catch {
          if (session === reading)
            reply({
              type: "threadlaunched",
              project: request.project,
              thread: null,
              error: "Thread started. Reconnect to load it.",
            });
          return;
        }
        if (reading === session)
          reply({ type: "threadlaunched", project: request.project, thread });
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
    } else if (request.type === "setsettlement") {
      reading = session;
      const feed = workspaceFeed;
      if (
        !reading?.canOperate ||
        feed?.watch !== request.watch ||
        !feed?.hasThread(request.thread)
      ) {
        reply({ type: "error", message: "This thread cannot be changed on this connection." });
        return;
      }
      const command = request.settled
        ? { type: "thread.settle" }
        : { type: "thread.unsettle", reason: "user" };
      const receipt = await Effect.runPromise(
        openEnvironmentRpc(reading).pipe(
          Effect.flatMap((rpc) =>
            rpc[ORCHESTRATION_V2_WS_METHODS.dispatchCommand]({
              ...command,
              threadId: ThreadId.make(request.thread),
              commandId: CommandId.make(request.operation),
            }),
          ),
          Effect.scoped,
          Effect.timeout("15 seconds"),
          Effect.provide(reading.http),
        ),
        { signal: reading.signal },
      );
      if (request.advance && receipt.sequence !== undefined) feed.publishThrough(receipt.sequence);
      reply({ type: "settlementchanged", watch: request.watch, sequence: receipt.sequence });
      return;
    } else if (request.type === "renamethread") {
      reading = session;
      const feed = workspaceFeed;
      const result = { type: "threadrenamed", watch: request.watch };
      if (
        !reading?.canOperate ||
        !feed ||
        feed.watch !== request.watch ||
        !feed.hasThread(request.thread) ||
        !request.title?.trim()
      ) {
        reply({ ...result, error: "This thread cannot be renamed on this connection." });
        return;
      }
      try {
        await Effect.runPromise(
          openEnvironmentRpc(reading).pipe(
            Effect.flatMap((rpc) =>
              rpc[ORCHESTRATION_V2_WS_METHODS.dispatchCommand]({
                type: "thread.metadata.update",
                threadId: ThreadId.make(request.thread),
                commandId: CommandId.make(request.operation),
                title: request.title.trim(),
              }),
            ),
            Effect.scoped,
            Effect.timeout("15 seconds"),
            Effect.provide(reading.http),
          ),
          { signal: reading.signal },
        );
        reply(result);
      } catch {
        reply({
          ...result,
          error: "Couldn’t confirm the thread rename. Refresh to check the title.",
        });
      }
      return;
    } else if (request.type === "setthreadmodel" && session) {
      reading = session;
      const record = watching;
      if (
        !reading.canOperate ||
        !record ||
        record.owner !== reading ||
        record.id !== request.watch ||
        request.thread !== record.thread
      )
        throw new ModelUnavailable({ message: "Reopen this thread to choose its model." });
      await Effect.runPromise(
        setThreadModel(reading, request, record.state.projection.thread).pipe(
          Effect.scoped,
          Effect.timeout("20 seconds"),
          Effect.provide(reading.http),
        ),
        { signal: reading.signal },
      );
      if (session === reading)
        reply({
          type: "modelsaved",
          thread: request.thread,
          instance: request.instance,
          model: request.model,
        });
      return;
    } else if (request.type === "sendmessage" && session) {
      reading = session;
      if (!reading.canOperate) throw new Error("Read-only session");
      await Effect.runPromise(
        sendThreadMessage(reading, request).pipe(
          Effect.scoped,
          Effect.timeout(request.files?.length ? "5 minutes" : "60 seconds"),
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
      if (watching && !workspace.view.threads.some((thread) => thread.id === watching.thread)) {
        leaveThread(watching.thread);
        stopWatching();
      }
      reply({
        type: "workspace",
        watch: request.id,
        clock_offset: workspace.clockOffset,
        sequence: workspace.sequence,
        ...workspace.view,
      });
      startWorkspaceFeed(reading, workspace, request.id);
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
    if (request?.type === "setappearance") {
      setAppearance(request.mode);
    } else if (request?.type === "setsidebarwidth" || request?.type === "setsidebaropen") {
      setSidebarLayout(request);
    } else if (request?.type === "visitthread") {
      if (visibleThread !== request.thread) leaveThread();
      visibleThread = request.thread;
      updateVisibleVisit();
    } else if (request?.type === "workspacehandled") {
      if (workspaceFeed?.watch === request.watch) workspaceFeed.handled();
    } else if (request?.type === "terminal") {
      const record = watching;
      if (record && record.id === request.watch) {
        if (request.action === "open") openTerminal(record, request, write);
        else if (record.terminal?.operation === request.operation) {
          if (request.action === "close") {
            record.terminal.close();
            record.terminal = undefined;
          } else record.terminal.input(request);
        }
      }
    } else if (request?.type === "threadpermissions" || request?.type === "threadmodels") {
      void loadComposerOptions(request, reply);
    } else if (request?.type === "inspectcommand") {
      void inspectCommand(request);
    } else if (request?.type === "imagehandled") {
      const sent = sentImages.get(request.operation);
      if (sent?.watch === request.watch) sent.handled.resolve();
    } else if (request?.type === "inspectimage") {
      void inspectImage(request);
    } else if (request?.type === "inspectfile" || request?.type === "inspecttool") {
      inspectDetail(request);
    } else if (request?.type === "stoprun") {
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
