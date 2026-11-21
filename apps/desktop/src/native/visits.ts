import { createThreadVisitTracker } from "@t3tools/client-runtime/state/thread-visits";
import {
  CommandId,
  ORCHESTRATION_V2_WS_METHODS,
  type EnvironmentId,
  type OrchestrationV2ThreadShell,
  type OrchestrationV2ThreadProjection,
} from "@t3tools/contracts";
import * as DateTime from "effect/DateTime";
import * as Effect from "effect/Effect";
import * as SecureRandom from "effect/crypto/SecureRandom";
import { Atom, AtomRegistry } from "effect/reactivity";
import type * as NodeSqlite from "node:sqlite";
import { openEnvironmentRpc } from "./environmentRpc.ts";
import type { Session } from "./projects.ts";

export type OpenVisitDatabase = () => NodeSqlite.DatabaseSync;

export function legacyThreadVisits(open: OpenVisitDatabase, environmentId: EnvironmentId) {
  let values: Map<string, string> | undefined;
  let database: NodeSqlite.DatabaseSync | undefined;
  const dirty = new Set<string>();
  function load() {
    if (values) return values;
    values = new Map();
    try {
      database = open();
      database.exec(
        "CREATE TABLE IF NOT EXISTS visits (environment TEXT, thread TEXT, visited TEXT NOT NULL, PRIMARY KEY(environment, thread))",
      );
      for (const { thread, visited } of database
        .prepare("SELECT thread, visited FROM visits WHERE environment = ?")
        .all(environmentId)) {
        if (typeof thread === "string" && typeof visited === "string") values.set(thread, visited);
      }
    } catch {
      database?.close();
      database = undefined;
    }
    return values;
  }
  function flush() {
    if (!database || !dirty.size) return;
    const save = database.prepare(
      "INSERT INTO visits VALUES(?, ?, ?) ON CONFLICT(environment, thread) DO UPDATE SET visited = MAX(visits.visited, excluded.visited)",
    );
    for (const thread of dirty) {
      try {
        save.run(environmentId, thread, values!.get(thread)!);
        dirty.delete(thread);
      } catch {
        return;
      }
    }
  }
  return {
    get: (thread: string) => load().get(thread),
    mark(thread: string, visited: string) {
      const marks = load();
      if ((marks.get(thread) ?? "") >= visited) return false;
      marks.set(thread, visited);
      dirty.add(thread);
      return true;
    },
    flush,
    close() {
      flush();
      database?.close();
      database = undefined;
      values = undefined;
    },
  };
}

export function visitInput(environmentId: EnvironmentId, shell: OrchestrationV2ThreadShell) {
  return {
    environmentId,
    id: shell.id,
    updatedAt: DateTime.formatIso(shell.updatedAt),
    lastVisitedAt:
      shell.lastVisitedAt == null ? shell.lastVisitedAt : DateTime.formatIso(shell.lastVisitedAt),
    latestRun: shell.latestRunCompletedAt
      ? { completedAt: DateTime.formatIso(shell.latestRunCompletedAt) }
      : null,
    legacy: shell.lastVisitedAt === undefined ? shell : undefined,
  };
}

type VisitInput = Omit<ReturnType<typeof visitInput>, "legacy">;

export function visitFromWindow(
  session: Session,
  projection: OrchestrationV2ThreadProjection,
): VisitInput {
  const { thread, runs } = projection;
  const completed = runs.at(-1)?.completedAt;
  return {
    environmentId: session.environmentId,
    id: thread.id,
    updatedAt: DateTime.formatIso(thread.updatedAt),
    lastVisitedAt: !session.visitTracking
      ? undefined
      : thread.lastVisitedAt && DateTime.formatIso(thread.lastVisitedAt),
    latestRun: completed ? { completedAt: DateTime.formatIso(completed) } : null,
  };
}

export function trackVisibleThread(
  session: Session,
  id: VisitInput["id"],
  seen: (at: string) => void,
) {
  const registry = AtomRegistry.make();
  const shell = Atom.make<VisitInput | null>(null);
  const generation = Atom.make(0);
  const tracker = createThreadVisitTracker({
    threadShellAtom: () => shell,
    sessionGenerationAtom: () => generation,
    markLocalVisit: (_, at) => seen(at),
    visit: {
      label: "nativeDesktop.visitThread",
      run: (_, { input }) =>
        session.canOperate && session.visitTracking
          ? Effect.gen(function* () {
              const random = yield* SecureRandom.SecureRandom;
              const rpc = yield* openEnvironmentRpc(session);
              yield* rpc[ORCHESTRATION_V2_WS_METHODS.dispatchCommand]({
                type: "thread.visit",
                commandId: CommandId.make(yield* random.uuidV4()),
                ...input,
              });
            }).pipe(
              Effect.scoped,
              Effect.timeout("5 seconds"),
              Effect.provide([session.http, SecureRandom.layerWebCrypto]),
            )
          : Effect.void,
    },
  });
  const unmount = registry.mount(tracker({ environmentId: session.environmentId, threadId: id }));
  return {
    id,
    update: (value: VisitInput | null) => registry.set(shell, value),
    reconnect: () => registry.update(generation, (n) => n + 1),
    close() {
      unmount();
      registry.dispose();
      session.legacyVisits?.flush();
    },
  };
}
