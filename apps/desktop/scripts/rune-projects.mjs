import { threadFindLabels, findInThread } from "../src/native/threadFind.ts";
import {
  archivedPageLabels,
  generalPageLabels,
  threadMenuLabels,
} from "../src/native/threadMenu.ts";
import { archivedThreadGroups } from "../src/native/archivedThreads.ts";
import { issueDetailReader } from "../src/native/issueDetail.ts";
import { issueLabels, workspaceIssues } from "../src/native/issues.ts";
import { pullRequestLabels, workspacePullRequests } from "../src/native/pullRequests.ts";
import { threadSnoozeMenu } from "../src/native/threadSnooze.ts";
import { savedViewLabels, savedViewStore } from "../src/native/savedViews.ts";
import { textPasteLimits } from "../src/native/attachments.ts";
import { watchUsageTotals } from "../src/native/usageTotals.ts";
import { usageLabels, usageProviders, watchUsage } from "../src/native/usage.ts";
import { openExternalUrl } from "./rune-external.mjs";
import {
  CommandId,
  DEFAULT_CLIENT_SETTINGS,
  ORCHESTRATION_V2_WS_METHODS,
  WS_METHODS,
  ThreadId,
} from "@t3tools/contracts";
import { MIN_APPEARANCE_CONTRAST, MAX_APPEARANCE_CONTRAST } from "@t3tools/contracts/settings";
import * as HostProcess from "@t3tools/shared/HostProcess";
import { readPairingAppLink, resolveRemotePairingTarget } from "@t3tools/shared/remote";
import { trackVisibleThread, visitFromWindow } from "../src/native/visits.ts";
import { watchSidebarProviders } from "../src/native/sidebarProviders.ts";
import { watchWorkspace } from "../src/native/workspaceFeed.ts";
import {
  readThreadLayout,
  setThreadDetailsVisibility,
  setThreadPanelMaximized,
  setThreadPanelWidth,
  threadDetailsLabels,
} from "../src/native/threadDetails.ts";
import { runeUpdates } from "./rune-updates.mjs";
import { selectLanguage, m } from "../src/i18n/catalog.ts";
import { msg } from "gt-i18n";
import { handleCodexAuthLink } from "./rune-codex-auth.mjs";
import { workspaceEditors } from "./rune-editors.mjs";
import { openTerminal } from "./rune-terminal.mjs";
import { readThreadModels, setThreadModel, ModelUnavailable } from "../src/native/models.ts";
import { PermissionUnavailable, readThreadPermissions } from "../src/native/permissions.ts";
import { resolveAddressBarInput, resolveSearchUrlTemplate } from "@t3tools/shared/preview";
import { browserLabels, openBrowserPanel } from "../src/native/browser.ts";
import { openEnvironmentRpc } from "../src/native/environmentRpc.ts";
import { workspaceDictation } from "./rune-dictation.mjs";
import { watchVoiceModels } from "../src/native/voiceModels.ts";
import { readClientSettings, patchClientSettings } from "../src/settings/DesktopClientSettings.ts";
import * as Logger from "effect/Logger";
import * as FileSystem from "effect/FileSystem";
import * as NodeFileSystem from "@effect/platform-node/NodeFileSystem";
import * as NodePathLayer from "@effect/platform-node/NodePath";
import { readAttachmentPreview } from "../src/native/attachmentPreview.ts";
import { writeFileStringAtomically } from "@t3tools/shared/atomicWrite";
import {
  downloadFile,
  downloadDestination,
  downloadQueueKey,
} from "../src/native/downloadAttachment.ts";
import { AttachmentUnavailable } from "../src/native/attachments.ts";
import { readImage, readRegistryIcon } from "../src/native/images.ts";
import * as Semaphore from "effect/Semaphore";
import * as Option from "effect/Option";
import { resolvePackagedRuneHomes } from "../src/app/DesktopStatePaths.ts";
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
  writePlanToWorkspace,
  readWorkspaceFiles,
  resolveWorkspaceMarkdownLink,
  workspaceMarkdownLink,
} from "../src/native/projects.ts";

const packaged = process.argv[2] === "--packaged";
const sourceBuild = process.argv[2] === "--source-build";
if (packaged && process.env.APPIMAGE && Effect.runSync(HostProcess.Platform) === "linux") {
  void import("./rune-appimage-links.mjs")
    .then(({ registerAppImageLinks }) => registerAppImageLinks(import.meta.dirname, process.env))
    .catch(() => process.stderr.write("Couldn’t register AppImage pairing links.\n"));
}
const currentVersion = import.meta.t3codeVersion ?? desktopPackage.version;
const buildStage = resolveDesktopAppStageLabel({
  isDevelopment: !packaged && !sourceBuild,
  appVersion: currentVersion,
}).toLowerCase();

let session;
let localSession;
let server;
let watching;
let menuWindow;
const pendingLaunches = new Map();
const attachmentDownloads = Semaphore.makeUnsafe(3);
const browserDownloads = Semaphore.makeUnsafe(3);
const directoryReads = Semaphore.makeUnsafe(4);
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
let composerFileSearch;
function stopWorkspaceFeed() {
  workspaceFeed?.controller.abort();
  workspaceFeed = undefined;
}

function startWorkspaceFeed(owner, initial, watch) {
  stopWorkspaceFeed();
  const controller = new AbortController();
  const signal = AbortSignal.any([owner.signal, controller.signal]);
  const editors = workspaceEditors({
    local: owner === localSession,
    session: owner,
    signal,
    preferred: readPreference("desktop-editor"),
    sshHintSeen: readPreference("desktop-remote-editor-hint") === "true",
    markSshHintSeen: () => savePreference("desktop-remote-editor-hint", true),
    save: (id) => savePreference("desktop-editor", id),
    publish: (catalog) => write({ type: "workspaceeditors", watch, catalog }),
  });
  const usage = watchUsage(owner, signal, (view) => write({ type: "usage", watch, ...view }));
  const usageTotals = watchUsageTotals(owner, signal, (view) =>
    write({ type: "usagetotals", watch, ...view }),
  );
  const hiddenProviders = new Set(usagePreferences().hiddenProviders);
  usage.filter(hiddenProviders);
  usageTotals.filter(hiddenProviders);
  let threadFindMode;
  let registryIcons = new Set();
  const configureProviders = watchSidebarProviders((instances, art) => {
    registryIcons = new Set(
      instances.flatMap((instance) => (instance.registry ? [instance.registry] : [])),
    );
    write({ type: "workspaceproviders", watch, instances, art });
  });
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
    (projects) => write({ type: "refreshfavicons", watch, projects }),
    () => activeVisit,
    (config, rpc) => {
      usage.configure(config, rpc);
      usageTotals.configure(config?.settings, rpc);
      editors.configure(config, rpc);
      configureProviders(config);
      if (config) {
        const mode = config.threadFind
          ? config.threadFindProgressive
            ? "stream"
            : "single"
          : "unavailable";
        if (mode !== threadFindMode) {
          threadFindMode = mode;
          write({
            type: "threadfindavailable",
            watch,
            labels: mode === "unavailable" ? null : threadFindLabels(),
          });
        }
      }
    },
    (ready) => write({ type: "composerfilesready", watch, ready }),
    (instance) => write({ type: "composerskillschanged", watch, instance }),
  );
  workspaceFeed = {
    watch,
    controller,
    editors,
    usage,
    usageTotals,
    dictation: workspaceDictation({
      owner,
      local: owner === localSession,
      signal,
      read: readPreference,
      save: savePreference,
      report: (event) => write({ ...event, watch }),
      settings: () => {
        const {
          speechLanguage,
          speechTranslateToEnglish,
          speechCustomWords,
          speechRemoveFillerWords,
          speechSilenceFiltering,
        } = clientSettings;
        return {
          speechLanguage,
          speechTranslateToEnglish,
          speechCustomWords,
          speechRemoveFillerWords,
          speechSilenceFiltering,
        };
      },
    }),
    voiceModels: watchVoiceModels(owner, signal, (catalog) =>
      write({ type: "voicemodels", watch, catalog }),
    ),
    readIssue: issueDetailReader(owner, signal, feed.activeRpc, (operation, reply) =>
      write({ type: "issuedetail", watch, operation, ...reply }),
    ),
    issues: workspaceIssues(owner, signal, feed.activeRpc, (operation, list, error) =>
      write({ type: "issues", watch, operation, list, error }),
    ),
    pullRequests: workspacePullRequests(owner, signal, feed.activeRpc, (operation, reply) =>
      write({ type: "pullrequests", watch, operation, ...reply }),
    ),
    threadFindMode: () => threadFindMode,
    hasRegistryIcon: (url) => registryIcons.has(url),
    ...feed,
  };
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

async function transferImage(owner, signal, result, read) {
  try {
    await Effect.runPromise(
      Effect.uninterruptibleMask((restore) =>
        restore(read.pipe(Effect.scoped, Effect.timeout("20 seconds"))).pipe(
          Effect.flatMap((data) => {
            if (signal.aborted) return Effect.void;
            // Bytes already sent occupy a decoder slot across watch changes until Rust acknowledges them.
            const sent = { watch: result.watch, handled: Promise.withResolvers() };
            sentImages.set(result.operation, sent);
            write({ ...result, data });
            return Effect.promise(() => sent.handled.promise).pipe(
              Effect.ensuring(Effect.sync(() => sentImages.delete(result.operation))),
            );
          }),
        ),
      ).pipe(imageReads.withPermit, Effect.provide(owner.http)),
      { signal },
    );
  } catch {
    if (!signal.aborted) write({ ...result, retryable: true });
  }
}

async function loadRegistryIcon(request) {
  const feed = workspaceFeed;
  const owner = session;
  if (!owner || !feed || feed.watch !== request.watch) return;
  const result = {
    type: "image",
    watch: request.watch,
    key: request.url,
    operation: request.operation,
    registry: true,
  };
  if (!feed.hasRegistryIcon(request.url)) return write(result);
  const signal = AbortSignal.any([owner.signal, feed.controller.signal]);
  await transferImage(owner, signal, result, readRegistryIcon(request.url));
}

async function loadProjectFavicon(request) {
  const feed = workspaceFeed;
  const owner = session;
  if (!owner || !feed || feed.watch !== request.watch) return;
  const result = {
    type: "image",
    watch: request.watch,
    key: request.project,
    operation: request.operation,
    favicon: true,
  };
  const target = feed.projectFavicon(request.project);
  if (!target || target.cwd !== request.cwd || (target.path ?? null) !== request.path)
    return write(result);
  const signal = AbortSignal.any([owner.signal, feed.controller.signal]);
  const read = Effect.suspend(() => readImage(owner, target, feed.activeRpc()));
  await transferImage(owner, signal, result, read);
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
    await transferImage(record.owner, signal, result, readImage(record.owner, resource));
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

async function handleEditor(request, reply) {
  const feed = workspaceFeed;
  if (!feed || feed.watch !== request.watch) return;
  const choose = request.type === "chooseeditor";
  try {
    const ok = choose
      ? feed.editors.choose(request.editor)
      : await feed.editors.open(request.editor, request.cwd);
    if (ok) {
      if (!choose && workspaceFeed === feed) {
        const editor = feed.editors.catalog().editors.find((e) => e.id === request.editor);
        if (editor)
          reply({
            type: "editoropened",
            watch: request.watch,
            message: m(msg("Opened in {editor}", { editor: editor.label })),
          });
      }
      return;
    }
  } catch {}
  if (workspaceFeed === feed) {
    reply({
      type: "editorerror",
      watch: request.watch,
      error: choose
        ? m(msg("Couldn’t save the preferred editor."))
        : m(msg("Couldn’t open the workspace in the selected editor.")),
    });
  }
}

async function refreshProvider(request, reply) {
  const feed = workspaceFeed;
  const result = { type: "providerrefreshed", watch: request.watch };
  if (!session || !feed || feed.watch !== request.watch) return;
  const refresh = feed.refreshProvider(request.key);
  try {
    if (!refresh) throw new Error("Provider status changed before refresh");
    await Effect.runPromise(refresh.pipe(Effect.timeout("60 seconds")), {
      signal: feed.controller.signal,
    });
    if (workspaceFeed === feed) reply(result);
  } catch {
    if (workspaceFeed === feed)
      reply({ ...result, error: m(msg("Couldn’t refresh provider status. Try again.")) });
  }
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

async function workspaceFiles(request, reply) {
  const record = watching;
  if (request.action === "resolve") {
    const link =
      record && record.id === request.watch && record.owner === session && session.canBrowse
        ? await Effect.runPromise(
            resolveWorkspaceMarkdownLink(
              record.owner,
              request.href,
              request.root,
              workspaceFeed?.activeRpc(),
            ).pipe(
              Effect.scoped,
              Effect.timeout("12 seconds"),
              Effect.provide(record.owner.http),
              Effect.orElseSucceed(() => workspaceMarkdownLink(request.href, request.root)),
            ),
            { signal: record.controller.signal },
          ).catch(() => null)
        : null;
    reply({ type: "workspacefilelink", operation: request.operation, link });
    return;
  }
  const filter = request.action === "filter";
  const search = request.action === "search" || filter;
  const list = request.action === "list";
  const channel = list ? "directories" : filter ? "fileFilter" : search ? "fileSearch" : "fileRead";
  const target = {
    type: list ? "fileentries" : filter ? "filteredfiles" : search ? "filematches" : "filesource",
    operation: request.operation,
    ...(list && { directory: request.directory }),
  };
  if (!record || record.id !== request.watch || record.owner !== session || !session.canBrowse) {
    return reply({ ...target, error: "Reopen this thread with file read access." });
  }
  const previous = record[channel];
  if (request.cancel) {
    if (previous?.operation === request.operation || search) previous?.controller.abort();
    return;
  }
  let job = previous;
  if (!list || previous?.operation !== request.operation) {
    previous?.controller.abort();
    job = { operation: request.operation, controller: new AbortController() };
    record[channel] = job;
  }
  const controller = job.controller;
  const signal = AbortSignal.any([
    record.controller.signal,
    record.owner.signal,
    controller.signal,
  ]);
  try {
    if (search && request.query) await NodeTimersPromises.setTimeout(120, undefined, { signal });
    const feed = workspaceFeed;
    const rpc = feed?.activeRpc();
    const rootChanged = () =>
      feed !== workspaceFeed ||
      (rpc && feed.activeRpc() !== rpc) ||
      (feed?.activeRpc() && feed.fileRoot(record.thread) !== request.root);
    const changed = {
      ...target,
      error: "The workspace changed. Refresh the thread and try again.",
    };
    if (rootChanged()) return reply(changed);
    const read = readWorkspaceFiles(record.owner, record.thread, request.root, request, rpc).pipe(
      Effect.scoped,
      Effect.timeout("12 seconds"),
      Effect.provide(record.owner.http),
    );
    const result = await Effect.runPromise(list ? directoryReads.withPermits(1)(read) : read, {
      signal,
    });
    if (!signal.aborted) reply(rootChanged() ? changed : { ...target, ...result });
  } catch (error) {
    if (!signal.aborted)
      reply({
        ...target,
        error:
          error?._tag === "ThreadWorkspaceChanged"
            ? "The workspace changed. Refresh the thread and try again."
            : error?.failure === "binary_file"
              ? "This file contains binary data."
              : list
                ? "Couldn’t load this folder. Refresh files to retry."
                : search
                  ? "Couldn’t search workspace files."
                  : "Couldn’t read this workspace file.",
      });
  } finally {
    if (!list && record[channel] === job) record[channel] = undefined;
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
const sourceHome = devHome || NodePath.resolve(desktop, "../../.t3/rune");
const configuredHome = process.env.T3CODE_HOME?.trim();
const homes = packaged
  ? resolvePackagedRuneHomes({
      homeDirectory: NodeOS.homedir(),
      joinPath: NodePath.join,
      t3Home: Option.fromNullishOr(configuredHome ? NodePath.resolve(configuredHome) : undefined),
      hasUserdata: (base) => {
        try {
          return (
            NodeFS.statSync(NodePath.join(base, "userdata"), { throwIfNoEntry: false }) !==
            undefined
          );
        } catch {
          // An unreadable profile belongs to the server to diagnose.
          return true;
        }
      },
    })
  : { server: sourceHome, preferences: sourceHome };
const releaseUpdate = runeUpdates({
  current: currentVersion,
  resources: packaged ? import.meta.dirname : undefined,
  cache: NodePath.join(homes.preferences, "update-cache"),
  signal: lifetime.signal,
  publish: write,
});
const clientSettingsPath = NodePath.join(homes.server, "userdata", "client-settings.json");
let clientSettings = DEFAULT_CLIENT_SETTINGS;
const snoozeMenu = threadSnoozeMenu();
function publishClientSettings(error) {
  write({
    type: "clientsettings",
    slash_skills: clientSettings.showSkillsInSlashMenu,
    reader_title: clientSettings.screenReaderWindowTitle,
    thread_labels: threadMenuLabels(),
    confirm_delete: clientSettings.confirmThreadDelete,
    confirm_archive: clientSettings.confirmThreadArchive,
    confirm_unpin: clientSettings.confirmThreadUnpin,
    settings_labels: {
      settings: m(msg("Settings")),
      appearance: m(msg("Appearance")),
      chat: m(msg("Chat")),
      archive: archivedPageLabels(),
      general: generalPageLabels(),
      voice: {
        title: m(msg("Voice")),
        dictation: {
          destination: m(msg("Transcribe with")),
          retry_revoke: m(msg("Retry turning off dictation")),
          off: m(msg("Off")),
          start: m(msg("Start voice input")),
          cancel: m(msg("Cancel voice input")),
          finish: m(msg("Finish voice input")),
          transcribing: m(msg("Transcribing…")),
          starting: m(msg("Starting…")),
        },
        microphone: {
          title: m(msg("Test microphone")),
          description: m(
            msg("Record ten seconds and play it back. The audio stays on this device."),
          ),
          record: m(msg("Record")),
          again: m(msg("Record again")),
          stop: m(msg("Stop recording")),
          opening: m(msg("Opening microphone…")),
          cancel: m(msg("Cancel microphone test")),
          play: m(msg("Play recording")),
          pause: m(msg("Pause recording")),
          discard: m(msg("Discard recording")),
          level: m(msg("Microphone input level")),
          input_error: m(msg("Could not access the microphone.")),
          empty_error: m(msg("No audio was recorded. Try again.")),
          play_error: m(msg("Could not play this recording. Try recording again.")),
        },
        models: m(msg("Models")),
        search: m(msg("Search transcription models")),
        empty: m(msg("No models match.")),
        download: m(msg("Download")),
        select: m(msg("Use model")),
        selected: m(msg("Selected")),
        cancel: m(msg("Cancel")),
        remove: m(msg("Delete")),
        refresh: m(msg("Refresh")),
        loading: m(msg("Loading…")),
        disconnected: m(msg("Connect an environment first.")),
      },
      composer: m(msg("Composer")),
      screen_reader: m(msg("Screen reader")),
      window_title: m(msg("Screen reader window title")),
      title_description: m(
        msg(
          "Start the window title with the thread's status, like Needs approval, and its name, so screen readers read them first when you switch apps. Taskbars, shared screens, and time trackers see it too.",
        ),
      ),
      title_reset: m(msg("Reset screen reader window title to default")),
      advanced: m(msg("Advanced")),
      slash_skills: m(msg("Show skills in slash menu")),
      slash_description: m(
        msg("Also include skills in the / command menu. Skills always appear when you type $."),
      ),
      slash_reset: m(msg("Reset skills in slash menu to default")),
      close: m(msg("Close settings")),
      search: m(msg("Search")),
      search_label: m(msg("Search settings")),
      no_results: m(msg("No settings found")),
      clear_search: m(msg("Clear settings search")),
    },
    chat_width: clientSettings.chatWidth,
    contrast: clientSettings.appearanceContrast,
    sans: clientSettings.fontFamilySans,
    code: clientSettings.fontFamilyCode,
    smoothing: clientSettings.fontSmoothing,
    error,
  });
}
let clientSettingsWrites = Effect.runPromise(
  FileSystem.FileSystem.use((fs) => readClientSettings(fs, clientSettingsPath)).pipe(
    Effect.provide(NodeFileSystem.layer),
    Effect.provideService(Logger.LogToStderr, true),
    Effect.catch(() => Effect.succeed(Option.none())),
  ),
).then(async (saved) => {
  clientSettings = Option.getOrElse(saved, () => DEFAULT_CLIENT_SETTINGS);
  await selectLanguage(clientSettings.language, [Intl.DateTimeFormat().resolvedOptions().locale]);
  publishClientSettings();
});

async function setClientSetting(request) {
  try {
    let changes;
    if (request.type === "setfontsmoothing" && typeof request.smoothing === "boolean") {
      changes = { fontSmoothing: request.smoothing };
    } else if (request.type === "setreadertitle" && typeof request.enabled === "boolean") {
      changes = { screenReaderWindowTitle: request.enabled };
    } else if (
      request.type === "setconfirmation" &&
      typeof request.enabled === "boolean" &&
      ["unpin", "archive", "delete"].includes(request.setting)
    ) {
      changes = {
        [{
          unpin: "confirmThreadUnpin",
          archive: "confirmThreadArchive",
          delete: "confirmThreadDelete",
        }[request.setting]]: request.enabled,
      };
    } else if (request.type === "setslashskills" && typeof request.enabled === "boolean") {
      changes = { showSkillsInSlashMenu: request.enabled };
    } else if (
      request.type === "setfontfamily" &&
      (request.slot === "sans" || request.slot === "code") &&
      typeof request.family === "string"
    ) {
      changes = { [request.slot === "sans" ? "fontFamilySans" : "fontFamilyCode"]: request.family };
    } else if (
      request.type === "setchatwidth" &&
      ["comfortable", "wide", "full"].includes(request.width)
    ) {
      changes = { chatWidth: request.width };
    } else if (
      request.type === "setcontrast" &&
      Number.isInteger(request.value) &&
      request.value >= MIN_APPEARANCE_CONTRAST &&
      request.value <= MAX_APPEARANCE_CONTRAST
    ) {
      changes = { appearanceContrast: request.value };
    } else throw new Error("Invalid client setting");
    clientSettings = await Effect.runPromise(
      patchClientSettings(clientSettingsPath, changes).pipe(
        Effect.provide(NodeFileSystem.layer),
        Effect.provide(NodePathLayer.layer),
        Effect.provideService(Logger.LogToStderr, true),
      ),
    );
    publishClientSettings();
  } catch {
    publishClientSettings(m(msg("Couldn’t save this setting on this computer.")));
  }
}

function readPreference(name) {
  try {
    return NodeFS.readFileSync(NodePath.join(homes.preferences, name), "utf8").trim();
  } catch (error) {
    if (error.code !== "ENOENT") process.stderr.write(`Couldn’t read ${name}.\n`);
    return undefined;
  }
}

function savePreference(name, value) {
  const path = NodePath.join(homes.preferences, name);
  if (value === null) {
    NodeFS.rmSync(path, { force: true });
    return;
  }
  const temp = path + "." + NodeCrypto.randomUUID();
  NodeFS.mkdirSync(homes.preferences, { recursive: true });
  try {
    NodeFS.writeFileSync(temp, String(value) + "\n", { mode: 0o600, flag: "wx" });
    NodeFS.renameSync(temp, path);
  } finally {
    NodeFS.rmSync(temp, { force: true });
  }
}

function validUsagePreferences(value) {
  return (
    ["cost", "tokens", "limits"].includes(value?.metric) &&
    [1, 7, 30, 90].includes(value.days) &&
    (value.hiddenProviders === undefined ||
      (Array.isArray(value.hiddenProviders) &&
        value.hiddenProviders.every((id) => usageProviders.includes(id))))
  );
}
const savedViews = savedViewStore(
  () => readPreference("desktop-saved-views"),
  (value) => savePreference("desktop-saved-views", value),
);
let unsavedUsagePreferences;
function usagePreferences() {
  if (unsavedUsagePreferences) return unsavedUsagePreferences;
  try {
    const saved = JSON.parse(readPreference("desktop-usage") ?? "null");
    if (validUsagePreferences(saved)) return saved;
  } catch {
    process.stderr.write("Couldn’t read usage preferences.\n");
  }
  return { metric: "limits", days: 30 };
}

function validAppearance(mode) {
  return mode === "system" || mode === "light" || mode === "dark";
}
const savedZoom = Number(readPreference("desktop-zoom"));
if (savedZoom >= 0.25 && savedZoom <= 5) write({ type: "windowzoom", factor: savedZoom });

const savedAppearance = readPreference("desktop-appearance");
let appearance = validAppearance(savedAppearance) ? savedAppearance : "system";
write({ type: "appearance", mode: appearance });

if (Effect.runSync(HostProcess.Platform) === "linux") {
  const fallbackDark = process.env.GTK_THEME?.split(":")[1] === "dark";
  const publish = (scheme) =>
    write({ type: "systemappearance", dark: scheme === null ? fallbackDark : scheme === "dark" });
  publish(null);
  void import("../src/app/LinuxColorScheme.ts")
    .then(({ watchLinuxColorScheme }) => {
      if (lifetime.signal.aborted) return;
      const watcher = watchLinuxColorScheme(publish);
      lifetime.signal.addEventListener("abort", watcher.stop, { once: true });
    })
    .catch(() => {});
}

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

function openDesktopDatabase(name) {
  NodeFS.mkdirSync(homes.preferences, { recursive: true });
  return new NodeSqlite.DatabaseSync(NodePath.join(homes.preferences, name), { timeout: 1000 });
}
const openVisitDatabase = () => openDesktopDatabase("desktop-visits.sqlite");
const openLayoutDatabase = () => openDesktopDatabase("desktop-layout.sqlite");

function localServer() {
  if (!server) {
    const cwd = packaged
      ? NodePath.join(import.meta.dirname, "server")
      : NodePath.resolve(desktop, "../server");
    const started = startLocalServer({
      packaged,
      version: import.meta.t3codeVersion ?? desktopPackage.version,
      cwd,
      entryPath: NodePath.join(cwd, packaged || sourceBuild ? "dist/bin.mjs" : "src/bin.ts"),
      t3Home: homes.server,
      ...(homes.preferences !== homes.server && { previousSettingsHome: homes.preferences }),
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
  if (type === "pair")
    return "Couldn’t pair with this environment. Check the pairing link and try again.";
  if (session)
    return type === "thread"
      ? "Couldn’t load this conversation. Try again."
      : "Couldn’t load this workspace. Check the connection and try again.";
  if (type === "local")
    return isLocalServerStartupError(error)
      ? error.message
      : "Couldn’t start the local environment. Try again or paste a pairing link.";
  return "Couldn’t load this workspace. Check the connection and try again.";
}

async function prepareDraft(owner, project, signal, reply) {
  const options = await Effect.runPromise(
    readLaunchOptions(
      owner,
      project,
      owner === session ? workspaceFeed?.draftConfig() : undefined,
    ).pipe(Effect.timeout("20 seconds"), Effect.scoped, Effect.provide(owner.http)),
    { signal },
  );
  if (owner === session && !signal.aborted) reply({ type: "launchoptions", ...options });
}

let landingRead;
async function prepareLanding(project, reply) {
  landingRead?.abort();
  landingRead = undefined;
  if (typeof project !== "string" || !session?.canOperate || !workspaceFeed) return;
  const controller = new AbortController();
  landingRead = controller;
  const signal = AbortSignal.any([
    controller.signal,
    workspaceFeed.controller.signal,
    session.signal,
  ]);
  try {
    await prepareDraft(session, project, signal, reply);
  } catch (error) {
    if (!signal.aborted)
      reply({ type: "error", message: requestError(error, "preparethread", false) });
  } finally {
    if (landingRead === controller) landingRead = undefined;
  }
}

function timelineFile(request) {
  const record = watching;
  if (!record || record.id !== request.watch || record.owner !== session) return;
  const row = record.state.rows[record.state.indices.get(request.key)];
  return row?.attachments.find((a) => a.file?.id === request.attachment)?.file;
}

async function previewAttachment(request, reply) {
  const record = watching;
  const result = { type: "filesource", operation: request.operation };
  if (!record || record.id !== request.watch) {
    if (!request.cancel) reply({ ...result, error: m(msg("The attachment is unavailable.")) });
    return;
  }
  if (request.cancel) {
    if (record.attachmentPreview?.operation === request.operation)
      record.attachmentPreview.controller.abort();
    return;
  }
  record.attachmentPreview?.controller.abort();
  const file = timelineFile(request);
  if (!file) return reply({ ...result, error: m(msg("The attachment is unavailable.")) });
  const controller = new AbortController();
  record.attachmentPreview = { operation: request.operation, controller };
  const signal = AbortSignal.any([
    record.controller.signal,
    record.owner.signal,
    controller.signal,
  ]);
  try {
    const source = await Effect.runPromise(
      openEnvironmentRpc(record.owner).pipe(
        Effect.flatMap((rpc) =>
          rpc[WS_METHODS.assetsCreateUrl]({
            resource: { _tag: "attachment", attachmentId: file.id, disposition: "inline" },
          }),
        ),
        Effect.flatMap(({ relativeUrl }) =>
          readAttachmentPreview(record.owner.httpBaseUrl, relativeUrl, file.bytes),
        ),
        Effect.scoped,
        Effect.timeout("30 seconds"),
        Effect.provide(record.owner.http),
      ),
      { signal },
    );
    if (!signal.aborted && timelineFile(request)?.id === file.id)
      reply({ ...result, contents: source.text, truncated: source.truncated });
    else if (!controller.signal.aborted)
      reply({ ...result, error: m(msg("The attachment is unavailable.")) });
  } catch {
    if (!controller.signal.aborted)
      reply({ ...result, error: m(msg("Could not load this file.")) });
  } finally {
    if (record.attachmentPreview?.controller === controller) record.attachmentPreview = undefined;
  }
}

function attachmentSave(request) {
  const record = watching;
  const file = timelineFile(request);
  if (!file) return;
  const owner = record.owner;
  return ({ path }, reply) =>
    saveEnvironmentFile(
      owner,
      openEnvironmentRpc(owner).pipe(
        Effect.flatMap((rpc) =>
          rpc[WS_METHODS.assetsCreateUrl]({
            resource: {
              _tag: "attachment",
              attachmentId: file.id,
              disposition: "attachment",
            },
          }),
        ),
        Effect.flatMap(({ relativeUrl }) =>
          downloadFile(owner.httpBaseUrl, relativeUrl, file.bytes, path),
        ),
        Effect.timeout("5 minutes"),
        attachmentDownloads.withPermits(1),
      ),
      reply,
    );
}

function browserFileSave(request) {
  const record = watching;
  if (
    !record ||
    record.id !== request.watch ||
    record.owner !== session ||
    !record.browser ||
    record.browser.operation !== request.operation
  )
    return;
  const file = record.browser.panel.download(request.tab, request.download);
  if (!file) return;
  return ({ path }, reply) =>
    saveEnvironmentFile(record.owner, file.save(path).pipe(browserDownloads.withPermits(1)), reply);
}

async function saveEnvironmentFile(owner, transfer, reply) {
  try {
    await Effect.runPromise(
      transfer.pipe(
        Effect.scoped,
        Effect.provide(owner.http),
        Effect.provide(NodeFileSystem.layer),
        Effect.provide(NodePathLayer.layer),
      ),
      { signal: owner.signal },
    );
    reply({ type: "downloaded" });
  } catch {
    reply({ type: "downloaded", error: m(msg("Could not save file")) });
  }
}

async function downloadPlan(request, reply) {
  try {
    await Effect.runPromise(
      writeFileStringAtomically({ filePath: request.path, contents: request.contents }).pipe(
        Effect.provide(NodeFileSystem.layer),
        Effect.provide(NodePathLayer.layer),
      ),
      { signal: lifetime.signal },
    );
    reply({ type: "downloaded" });
  } catch {
    reply({
      type: "downloaded",
      error: "Couldn’t write the Markdown file. Choose another location and try again.",
    });
  }
}

async function savePlan(request, reply) {
  const record = watching;
  const result = { type: "plansaved", watch: request.watch };
  const row = record?.state.rows[record.state.indices.get(request.key)];
  if (
    !session?.canWriteFiles ||
    !record ||
    record.id !== request.watch ||
    record.owner !== session ||
    row?.role !== "plan"
  ) {
    reply({ ...result, error: "This plan or workspace is no longer available for saving." });
    return;
  }
  try {
    const owner = record.owner;
    await Effect.runPromise(
      writePlanToWorkspace(owner, record.thread, request.root, request.path, row.body).pipe(
        Effect.scoped,
        Effect.timeout("20 seconds"),
        Effect.provide(owner.http),
      ),
      { signal: record.controller.signal },
    );
    reply(result);
  } catch {
    reply({
      ...result,
      error: "Couldn’t save this plan. Check the workspace and relative path, then try again.",
    });
  }
}

async function handleWorkspaceRequest(request, reply) {
  if (lifetime.signal.aborted) return;
  let reading;
  let editing = false;
  let initialWorkspace;
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
      const host = request.type === "local" ? localServer() : undefined;
      const connected = host
        ? await host.connect(lifetime.signal, openVisitDatabase)
        : await pairWithEnvironment(request.link, lifetime.signal, openVisitDatabase);
      try {
        await clientSettingsWrites;
        initialWorkspace = await readWorkspace(connected);
        if (host && server !== host) throw new Error("Local server stopped");
      } catch (error) {
        connected.legacyVisits?.close();
        throw error;
      }
      leaveThread();
      session?.legacyVisits?.close();
      session = connected;
      localSession = host ? connected : undefined;
      stopWorkspaceFeed();
      stopWatching();
      pendingLaunches.clear();
      let layout;
      try {
        layout = readThreadLayout(openLayoutDatabase, session.environmentId);
      } catch {
        process.stderr.write("Couldn’t read the desktop layout.\n");
      }
      reply({
        type: "connected",
        label: session.label,
        composer_hints: {
          message: m(msg("Ask anything. @ files, $ skills, / commands")),
          type_answer: m(msg("Type your answer")),
          choose_option: m(msg("Choose an option above")),
          optional_answer: m(
            msg("Type your own answer, or leave this blank to use the selected option"),
          ),
        },
        mention_labels: {
          search: m(msg("Search files")),
          commands: m(msg("Commands")),
          switch_model: m(msg("Switch response model for this thread")),
          skills: m(msg("Skills")),
          no_skills: m(msg("No results found.")),
          searching: m(msg("Searching…")),
          empty: m(msg("No files found.")),
        },
        attachment_labels: {
          too_large: m(msg("Pasted text is too large to attach")),
          remove_some: m(msg("Remove some text or an attachment, then paste again.")),
          attachment: m(msg("Attachment")),
          copy: m(msg("Copy contents")),
          copy_message: m(msg("Copy message")),
          copy_preview: m(msg("Copy preview")),
          source: m(msg("Show source")),
          rendered: m(msg("Show rendered markdown")),
          no_preview: m(msg("No preview for this file")),
          loading: m(msg("Loading file")),
          file_close: m(msg("Close")),
          truncated: m(msg("Preview limited to the first 1 MB. Save the file to read it in full.")),
          retry: m(msg("Try again")),

          download: m(msg("Download {label}", { label: "{label}" })),
          pasted: m(msg("Pasted text")),
          file: m(msg("File", { $context: "file type badge when the name has no extension" })),
          close: m(msg("Close image preview")),
          preview: m(msg("Preview {name}", { name: "{name}" })),
          remove: m(msg("Remove {name}", { name: "{name}" })),
        },
        panel_width: {
          widths: layout?.widths ?? {},
          maximized: layout?.maximized ?? [],
          resize: m(msg("Resize right panel")),
          maximize: m(msg("Maximize panel")),
          restore: m(msg("Restore panel size")),
        },
        browser_labels: session.browserAvailable ? browserLabels() : undefined,
        issues_labels: session.canReadIssues ? issueLabels() : undefined,
        pull_request_labels: session.canReadPullRequests ? pullRequestLabels() : undefined,
        usage_labels: usageLabels(),
        usage_preferences: usagePreferences(),
        saved_view_labels: savedViewLabels(),
        saved_views: savedViews.load(),
        update_labels: {
          check: m(msg("Check for updates")),
          checking: m(msg("Checking for updates…")),
          available: m(msg("Update ready to download")),
          downloading: m(msg("Downloading update")),
          downloaded: m(msg("Update downloaded")),
          restart: m(msg("Restart to update")),
          installing: m(msg("Installing…")),
        },
        details: {
          hidden: layout?.hidden,
          machine: initialWorkspace.machine,
          windows: session.platform === "windows",
          environment: clientSettings.threadDetailsItems.environment !== false,
          openIn: clientSettings.threadDetailsItems.openIn !== false,
          scripts: clientSettings.threadDetailsItems.scripts !== false,
          scriptWrite: session.canWriteSettings && session.canOperate,
          gitActions: clientSettings.threadDetailsItems.gitActions !== false,
          gitWrite: session.canWriteGit,
          labels: threadDetailsLabels(),
        },
        local: localSession?.sameFilesystem ?? false,
        can_operate: session.canOperate,
        can_read_files: session.canBrowse,
        text_paste: textPasteLimits(session),
        can_write_files: session.canWriteFiles,
        can_pin: session.canPin,
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
        await prepareDraft(
          reading,
          request.type === "preparescratch" ? null : request.project,
          reading.signal,
          reply,
        );
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
    } else if (request.type === "archivedthreads") {
      reading = session;
      try {
        if (!reading) throw new Error("No connection");
        const snapshot = await Effect.runPromise(
          openEnvironmentRpc(reading).pipe(
            Effect.flatMap((rpc) => rpc[ORCHESTRATION_V2_WS_METHODS.getArchivedShellSnapshot]({})),
            Effect.scoped,
            Effect.timeout("15 seconds"),
            Effect.provide(reading.http),
          ),
          { signal: reading.signal },
        );
        reply({
          type: "archivedthreads",
          operation: request.operation,
          groups: archivedThreadGroups(snapshot),
        });
      } catch {
        reply({
          type: "archivedthreads",
          operation: request.operation,
          groups: [],
          error: m(msg("Could not load archived threads")),
        });
      }
      return;
    } else if (request.type === "snoozepresets") {
      reading = session;
      const feed = workspaceFeed;
      const result = { type: "snoozepresets", operation: request.operation, presets: [] };
      if (
        !reading?.canOperate ||
        feed?.watch !== request.watch ||
        !feed.hasThread(request.thread)
      ) {
        reply(result);
        return;
      }
      const config = await Effect.runPromise(
        openEnvironmentRpc(reading).pipe(
          Effect.flatMap((rpc) => rpc[WS_METHODS.serverGetConfig]({})),
          Effect.scoped,
          Effect.timeout("15 seconds"),
          Effect.provide(reading.http),
        ),
        { signal: reading.signal },
      ).catch(() => undefined);
      reply({
        ...result,
        presets: snoozeMenu.open(request.thread, config?.settings.snoozePresets, new Date()),
      });
      return;
    } else if (request.type === "snooze" || request.type === "unsnooze") {
      reading = session;
      const feed = workspaceFeed;
      const result = { type: "threadedited", watch: request.watch, operation: request.operation };
      const snoozedUntil =
        request.type === "snooze"
          ? snoozeMenu.pick(request.thread, request.preset, new Date())
          : null;
      if (
        !reading?.canOperate ||
        !feed ||
        feed.watch !== request.watch ||
        !feed.hasThread(request.thread)
      ) {
        reply({ ...result, error: "This thread cannot be changed on this connection." });
        return;
      }
      if (snoozedUntil === undefined) {
        reply({ ...result, error: m(msg("That snooze time has passed. Choose another time.")) });
        return;
      }
      try {
        await Effect.runPromise(
          openEnvironmentRpc(reading).pipe(
            Effect.flatMap((rpc) =>
              rpc[ORCHESTRATION_V2_WS_METHODS.dispatchCommand]({
                ...(snoozedUntil === null
                  ? { type: "thread.unsnooze", reason: "user" }
                  : { type: "thread.snooze", snoozedUntil }),
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
        reply(result);
      } catch {
        reply({
          ...result,
          error:
            snoozedUntil === null
              ? m(msg("Could not wake thread"))
              : m(msg("Could not snooze thread")),
        });
      }
      return;
    } else if (
      request.type === "markunread" ||
      request.type === "deletethread" ||
      request.type === "archivethread" ||
      request.type === "unarchivethread"
    ) {
      reading = session;
      const feed = workspaceFeed;
      const result = { type: "threadedited", watch: request.watch };
      // Archived threads have left the live feed, so they are checked by the server instead.
      const live = request.type === "markunread" || request.type === "archivethread";
      if (
        !reading?.canOperate ||
        !feed ||
        feed.watch !== request.watch ||
        (live && !feed.hasThread(request.thread))
      ) {
        reply({ ...result, error: "This thread cannot be changed on this connection." });
        return;
      }
      try {
        await Effect.runPromise(
          openEnvironmentRpc(reading).pipe(
            Effect.flatMap((rpc) =>
              rpc[ORCHESTRATION_V2_WS_METHODS.dispatchCommand]({
                type: {
                  markunread: "thread.mark-unread",
                  deletethread: "thread.delete",
                  archivethread: "thread.archive",
                  unarchivethread: "thread.unarchive",
                }[request.type],
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
        reply(result);
      } catch {
        // Like the web client, a failed mark-unread stays silent.
        const failure = {
          deletethread: m(msg("Failed to delete thread")),
          archivethread: m(msg("Failed to archive thread")),
          unarchivethread: m(msg("Failed to unarchive thread")),
        }[request.type];
        reply(failure ? { ...result, error: failure } : result);
      }
      return;
    } else if (request.type === "renamethread" || request.type === "setpin") {
      reading = session;
      const feed = workspaceFeed;
      const result = { type: "threadedited", watch: request.watch };
      const rename = request.type === "renamethread";
      if (
        !reading?.canOperate ||
        !feed ||
        feed.watch !== request.watch ||
        !feed.hasThread(request.thread) ||
        (rename ? !request.title?.trim() : !reading.canPin || typeof request.pinned !== "boolean")
      ) {
        reply({ ...result, error: "This thread cannot be changed on this connection." });
        return;
      }
      try {
        const orderKey = !rename && request.pinned ? feed.nextPinOrderKey() : undefined;
        const command = rename
          ? { type: "thread.metadata.update", title: request.title.trim() }
          : request.pinned
            ? { type: "thread.pin", orderKey }
            : { type: "thread.unpin" };
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
        if (orderKey !== undefined) feed.pinCommitted(orderKey, receipt.sequence);
        reply(result);
      } catch {
        reply({
          ...result,
          error: rename
            ? "Couldn’t confirm the thread rename. Refresh to check the title."
            : "Couldn’t confirm the pin change. Refresh to check the thread.",
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
    const workspace = initialWorkspace ?? (await readWorkspace(reading));
    if (session === reading) {
      if (watching && !workspace.view.threads.some((thread) => thread.id === watching.thread)) {
        leaveThread(watching.thread);
        stopWatching();
      }
      startWorkspaceFeed(reading, workspace, request.id);
      reply({
        type: "workspace",
        environment_id: reading.environmentId,
        editors: workspaceFeed.editors.catalog(),
        dictation: workspaceFeed.dictation.choice(),
        landing_project: workspaceFeed.initialLanding,
        watch: request.id,
        clock_offset: workspace.clockOffset,
        sequence: workspace.sequence,
        ...workspace.view,
      });
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
  } finally {
    // The old request still owns clipboard files until its file-backed upload has settled.
    if (
      reading &&
      reading !== session &&
      request.files?.length &&
      (request.type === "sendmessage" || request.type === "launchthread")
    ) {
      reply({ type: "unchanged" });
    }
  }
}

try {
  let workspaceRequests = Promise.resolve();
  const downloads = new Map();
  let resolvingDownloads = Promise.resolve();
  async function queueDownload(request, reply, save) {
    let path, key;
    try {
      [path, key] = await Effect.runPromise(
        downloadDestination(request.path).pipe(
          Effect.flatMap((path) =>
            downloadQueueKey(path, downloads.keys()).pipe(Effect.map((key) => [path, key])),
          ),
          Effect.provide(NodeFileSystem.layer),
          Effect.provide(NodePathLayer.layer),
        ),
        { signal: lifetime.signal },
      );
    } catch {
      reply({ type: "downloaded", error: m(msg("Could not save file")) });
      return;
    }
    const previous = downloads.get(key) ?? Promise.resolve();
    const pending = previous.then(() => save({ ...request, path }, reply));
    downloads.set(key, pending);
    void pending.finally(() => {
      if (downloads.get(key) === pending) downloads.delete(key);
    });
  }

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
    if (request?.type === "setdictation" || request?.type === "dictate") {
      const feed = workspaceFeed;
      if (feed && feed.watch === request.watch) {
        if (request.type === "setdictation") feed.dictation.select(request.enabled === true);
        else if (request.action === "start")
          void feed.dictation.start(request.operation, request.draft);
        else if (request.action === "insert")
          feed.dictation.insert(request.operation, request.draft);
        else feed.dictation.finish(request.operation, request.action === "cancel");
      }
      continue;
    }
    if (request?.type === "voicemodels" || request?.type === "changevoicemodel") {
      if (workspaceFeed && workspaceFeed.watch === request.watch) {
        if (request.type === "voicemodels") workspaceFeed.voiceModels.show(request.open);
        else void workspaceFeed.voiceModels.change(request.action, request.model);
      }
      continue;
    }
    if (request?.type === "composerfiles") {
      composerFileSearch?.abort();
      const feed = workspaceFeed;
      if (!feed || feed.watch !== request.watch || request.query == null) continue;
      const controller = new AbortController();
      composerFileSearch = controller;
      const signal = AbortSignal.any([feed.controller.signal, controller.signal]);
      const result = (entries) => {
        if (!signal.aborted)
          reply({ type: "composerfiles", operation: request.operation, entries });
      };
      void Effect.runPromise(
        feed
          .searchComposerEntries(
            request.draft,
            request.target,
            request.root,
            request.query,
            request.instance,
            request.slash
              ? { model: request.model ?? null, showSkills: clientSettings.showSkillsInSlashMenu }
              : undefined,
          )
          .pipe(Effect.timeout("12 seconds")),
        { signal },
      ).then(result, () => result(null));
      continue;
    }
    if (request?.type === "findthread") {
      const record = watching;
      const respond = (result) =>
        write({ type: "threadfind", watch: request.watch, operation: request.operation, result });
      const mode = workspaceFeed?.threadFindMode();
      if (!record || record.id !== request.watch || (mode !== "stream" && mode !== "single")) {
        if (request.query) respond(null);
        continue;
      }
      record.find?.abort();
      record.find = undefined;
      if (request.query) {
        const controller = new AbortController();
        record.find = controller;
        const signal = AbortSignal.any([
          record.owner.signal,
          record.controller.signal,
          controller.signal,
        ]);
        void Effect.runPromise(
          findInThread(
            record.owner,
            mode === "stream",
            record.thread,
            request,
            workspaceFeed?.activeRpc(),
            respond,
          ).pipe(Effect.scoped, Effect.provide(record.owner.http)),
          { signal },
        ).catch(() => {
          if (!signal.aborted) respond(null);
        });
      }
      continue;
    }
    if (request?.type === "issuedetail") {
      const feed = workspaceFeed;
      if (feed?.watch === request.watch)
        void feed.readIssue(request.operation, request.reference, request.cursor);
      continue;
    }
    if (request?.type === "issues") {
      const feed = workspaceFeed;
      if (feed?.watch === request.watch) {
        if (request.action === "close") feed.issues.close();
        else void feed.issues.request(request);
      }
      continue;
    }
    if (request?.type === "pullrequests") {
      const feed = workspaceFeed;
      if (feed?.watch === request.watch) {
        if (request.action === "close") feed.pullRequests.close();
        else void feed.pullRequests.request(request);
      }
      continue;
    }
    if (request?.type === "openurl") {
      const link = readPairingAppLink(request.url);
      if (link) {
        try {
          const { httpBaseUrl } = resolveRemotePairingTarget({ pairingUrl: link });
          write({ type: "pairingoffer", origin: new URL(httpBaseUrl).origin, link });
        } catch {
          // A malformed launch link must not interrupt the current environment.
        }
      } else {
        void handleCodexAuthLink(request.url, !packaged && !sourceBuild, lifetime.signal);
      }
      continue;
    }
    if (request?.type === "x11menu") {
      if (
        menuWindow?.id !== request.window &&
        Number.isInteger(request.window) &&
        request.window > 0
      ) {
        menuWindow?.controller.abort();
        const controller = new AbortController();
        menuWindow = { id: request.window, controller };
        const signal = AbortSignal.any([controller.signal, lifetime.signal]);
        void Promise.all([import("./rune-linux-menu.mjs"), clientSettingsWrites])
          .then(([menu]) =>
            menu.registerWindow(request.window, clientSettings.language, signal, (action) =>
              write({ type: "menuaction", action }),
            ),
          )
          .catch(() => {});
      }
      continue;
    }
    if (
      request?.type === "checkupdates" ||
      request?.type === "downloadupdate" ||
      request?.type === "installupdate" ||
      request?.type === "showupdate"
    ) {
      void releaseUpdate(request.type);
      continue;
    }
    if (request?.type === "setwindowzoom") {
      const factor = request.factor;
      if (Number.isFinite(factor) && factor >= 0.25 && factor <= 5) {
        try {
          savePreference("desktop-zoom", factor === 1 ? null : factor);
        } catch {
          process.stderr.write("Couldn’t save the desktop zoom.\n");
        }
      }
      continue;
    }
    if (request?.type === "setappearance") {
      setAppearance(request.mode);
    } else if (
      request?.type === "setfontsmoothing" ||
      request?.type === "setslashskills" ||
      request?.type === "setreadertitle" ||
      request?.type === "setconfirmation" ||
      request?.type === "setfontfamily" ||
      request?.type === "setchatwidth" ||
      request?.type === "setcontrast"
    ) {
      clientSettingsWrites = clientSettingsWrites.then(() => setClientSetting(request));
    } else if (request?.type === "openeditor" || request?.type === "chooseeditor") {
      void handleEditor(request, reply);
    } else if (request?.type === "setpanelwidth" || request?.type === "setpanelmaximized") {
      if (session && workspaceFeed?.watch === request.watch && typeof request.thread === "string") {
        try {
          if (request.type === "setpanelmaximized" && typeof request.maximized === "boolean") {
            setThreadPanelMaximized(
              openLayoutDatabase,
              session.environmentId,
              request.thread,
              request.maximized,
            );
          } else if (request.type === "setpanelwidth" && validWidth(request.width)) {
            setThreadPanelWidth(
              openLayoutDatabase,
              session.environmentId,
              request.thread,
              request.width,
            );
          }
        } catch {
          reply({
            type: "layoutsaveerror",
            watch: request.watch,
            error:
              request.type === "setpanelwidth"
                ? m(msg("Couldn’t save the panel width."))
                : m(msg("Couldn’t save the panel layout.")),
          });
        }
      }
    } else if (request?.type === "setdetailsvisible") {
      if (session && workspaceFeed?.watch === request.watch) {
        const { watch, target, draft, visible } = request;
        try {
          setThreadDetailsVisibility(
            openLayoutDatabase,
            session.environmentId,
            target,
            draft,
            visible,
          );
        } catch {
          reply({
            type: "layoutsaveerror",
            watch,
            error: m(msg("Couldn’t save thread details visibility.")),
          });
        }
      }
    } else if (request?.type === "setsidebarwidth" || request?.type === "setsidebaropen") {
      setSidebarLayout(request);
    } else if (request?.type === "visitthread") {
      if (visibleThread !== request.thread) leaveThread();
      visibleThread = request.thread;
      updateVisibleVisit();
    } else if (request?.type === "preparelanding") {
      void prepareLanding(request.project, reply);
    } else if (request?.type === "setsavedviews") {
      try {
        if (!savedViews.write(request.list, request.views))
          process.stderr.write("Ignored malformed saved views.\n");
      } catch {
        process.stderr.write("Couldn’t save views.\n");
      }
    } else if (request?.type === "setusagepreferences") {
      if (validUsagePreferences(request.preferences)) {
        const hidden = new Set(request.preferences.hiddenProviders);
        workspaceFeed?.usage.filter(hidden);
        workspaceFeed?.usageTotals.filter(hidden);
        try {
          savePreference("desktop-usage", JSON.stringify(request.preferences));
          unsavedUsagePreferences = undefined;
        } catch {
          unsavedUsagePreferences = request.preferences;
          process.stderr.write("Couldn’t save usage preferences.\n");
        }
      }
    } else if (request?.type === "usage") {
      const feed = workspaceFeed;
      if (!feed || feed.watch !== request.watch) continue;
      switch (request.action) {
        case "open":
          feed.usageTotals.close();
          feed.usage.select(true);
          break;
        case "summary":
          if (Number.isSafeInteger(request.selection) && [1, 7, 30, 90].includes(request.days)) {
            feed.usage.select(false);
            feed.usageTotals.select(request.selection, request.days);
          }
          break;
        case "refreshsummary":
          feed.usageTotals.refresh();
          break;
        case "close":
          feed.usageTotals.close();
          feed.usage.select(false);
          break;
        case "refresh":
          void feed.usage.refresh();
          break;
        case "manage":
          void feed.usage.manage(request.url, openExternalUrl);
          break;
      }
    } else if (request?.type === "watchprojectscripts") {
      const feed = workspaceFeed;
      if (feed && feed.watch === request.watch) {
        if (request.project && clientSettings.threadDetailsItems.scripts !== false) {
          feed.scripts.clear();
          feed.selectScriptTarget({ project: request.project, thread: request.thread ?? null });
          feed.scripts.select(request.project, (rows, available, imports) => {
            if (feed === workspaceFeed)
              reply({
                type: "projectscripts",
                watch: feed.watch,
                project: request.project,
                rows,
                available,
                imports,
              });
          });
        } else {
          feed.scripts.clear();
          feed.selectScriptTarget(undefined);
        }
      }
    } else if (request?.type === "refreshprojectfileactions") {
      if (workspaceFeed?.watch === request.watch) workspaceFeed.fileActions.reconcile(true);
    } else if (request?.type === "saveprojectscript") {
      const feed = workspaceFeed;
      if (
        feed?.watch === request.watch &&
        session?.canOperate &&
        session?.canWriteSettings &&
        clientSettings.threadDetailsItems.scripts !== false
      ) {
        void feed.scripts
          .save(
            request.project,
            request.key,
            request.input,
            feed.activeRpc,
            feed.controller.signal,
            request.from_file ? feed.fileActions.current : undefined,
          )
          .then((warning) => reply({ type: "projectscriptsaved", watch: request.watch, warning }))
          .catch((error) =>
            reply({
              type: "projectscriptsaved",
              watch: request.watch,
              error: error.message || m(msg("Failed to save action.")),
            }),
          );
      } else
        reply({
          type: "projectscriptsaved",
          watch: request.watch,
          error: m(msg("This connection cannot change project actions.")),
        });
    } else if (request?.type === "watchgit") {
      if (workspaceFeed?.watch === request.watch)
        workspaceFeed.git.select(request.id, request.selection ?? null, (nonRepo) =>
          reply({ type: "gitstatus", watch: request.watch, non_repo: nonRepo }),
        );
    } else if (request?.type === "initgit") {
      const feed = workspaceFeed;
      if (feed && feed.watch === request.watch)
        void feed.git
          .initialize(request.subscription)
          .catch(() => false)
          .then((ok) => {
            if (workspaceFeed === feed)
              reply({
                type: "gitinitialized",
                watch: request.watch,
                error: ok ? undefined : m(msg("Git initialization failed")),
              });
          });
    } else if (request?.type === "watchproviderstatus") {
      if (session && workspaceFeed?.watch === request.watch)
        workspaceFeed.selectBanner(
          request.project ? { project: request.project, instance: request.instance } : null,
          (banner) => reply({ type: "providerstatus", watch: request.watch, banner }),
        );
    } else if (request?.type === "refreshprovider") {
      void refreshProvider(request, reply);
    } else if (request?.type === "watchdraftmodels") {
      if (session?.canOperate && workspaceFeed?.watch === request.watch)
        workspaceFeed.selectDraftModels(request.project ?? null, (catalog) =>
          reply({
            type: "draftmodels",
            watch: request.watch,
            project: request.project,
            ...catalog,
          }),
        );
    } else if (request?.type === "workspacehandled") {
      if (workspaceFeed?.watch === request.watch) workspaceFeed.handled();
    } else if (request?.type === "browser") {
      const record = watching;
      if (record && record.id === request.watch && record.owner.browserAvailable) {
        const { watch, operation } = request;
        if (request.action === "show") {
          record.browser?.panel.stop();
          const signal = AbortSignal.any([record.owner.signal, record.controller.signal]);
          const panel = openBrowserPanel(
            record.owner,
            record.thread,
            signal,
            (page) => write({ type: "browserpage", watch, operation, page }),
            (tab, sequence, jpeg) =>
              write({
                type: "browserframe",
                watch,
                operation,
                tab,
                sequence,
                data: Buffer.from(jpeg).toString("base64"),
              }),
            (tab, text) => write({ type: "browserclipboard", watch, operation, tab, text }),
            (input) => resolveAddressBarInput(input, resolveSearchUrlTemplate(clientSettings)),
            () => clientSettings.browserDefaultViewport,
          );
          record.browser = { operation, panel };
        } else if (record.browser?.operation === operation) {
          if (request.action === "openDownload") {
            void record.browser.panel.openDownload(request.tab, request.id, openExternalUrl);
          } else if (request.action === "hide") {
            record.browser.panel.stop();
            record.browser = undefined;
          } else {
            void record.browser.panel.request(request);
          }
        }
      }
    } else if (request?.type === "terminal") {
      const record = watching;
      if (record && record.id === request.watch) {
        if (request.action === "open") {
          const resolveScript = () => {
            const feed = workspaceFeed;
            if (
              !feed?.activeRpc() ||
              !record.synchronized ||
              !record.owner.canOperateTerminal ||
              clientSettings.threadDetailsItems.scripts === false ||
              feed.fileRoot(record.thread) !==
                (record.state.projection.thread.worktreePath ?? record.state.projectRoot)
            )
              return;
            return feed.scripts.lookup(record.state.projection.thread.projectId, request.script);
          };
          openTerminal(record, request, write, resolveScript);
        } else if (record.terminal?.operation === request.operation) {
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
    } else if (request?.type === "registryicon") {
      void loadRegistryIcon(request);
    } else if (request?.type === "projectfavicon") {
      void loadProjectFavicon(request);
    } else if (request?.type === "inspectimage") {
      void inspectImage(request);
    } else if (request?.type === "inspectfile" || request?.type === "inspecttool") {
      inspectDetail(request);
    } else if (request?.type === "workspacefiles") {
      void workspaceFiles(request, reply);
    } else if (request?.type === "previewattachment") {
      void previewAttachment(request, reply);
    } else if (request?.type === "saveplan") {
      void savePlan(request, reply);
    } else if (
      request?.type === "downloadattachment" ||
      request?.type === "downloadplan" ||
      request?.type === "downloadbrowserfile"
    ) {
      const save =
        request.type === "downloadplan"
          ? downloadPlan
          : request.type === "downloadbrowserfile"
            ? browserFileSave(request)
            : attachmentSave(request);
      if (save)
        resolvingDownloads = resolvingDownloads.then(() => queueDownload(request, reply, save));
      else reply({ type: "downloaded", error: m(msg("The attachment is unavailable.")) });
    } else if (request?.type === "stoprun") {
      void interruptRun(session, request, reply);
    } else {
      workspaceRequests = workspaceRequests.then(() => handleWorkspaceRequest(request, reply));
    }
  }
  await resolvingDownloads;
  await Promise.all([workspaceRequests, ...downloads.values(), clientSettingsWrites]);
} finally {
  close();
  await server?.stopped;
}
