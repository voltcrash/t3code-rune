import { WS_METHODS } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as Schedule from "effect/Schedule";
import * as Stream from "effect/Stream";
import type { openEnvironmentRpc } from "./environmentRpc.ts";

type Rpc = Effect.Success<ReturnType<typeof openEnvironmentRpc>>;
export type GitTarget = { draft: boolean; target: string; cwd: string };

export function watchWorkspaceGit(
  signal: AbortSignal,
  session: { canWriteGit: boolean; vcsBranchStatus: boolean },
  isCurrent: (target: GitTarget) => boolean,
) {
  let rpc: Rpc | undefined;
  let selected: { id: number; target: GitTarget; emit: (nonRepo: boolean) => void } | undefined;
  let active: AbortController | undefined;
  let nonRepo = false;
  let initializing = false;

  function stop() {
    active?.abort();
    active = undefined;
    if (nonRepo) selected?.emit(false);
    nonRepo = false;
    initializing = false;
  }

  function reconcile() {
    if (!rpc || !selected || !isCurrent(selected.target)) {
      stop();
      return;
    }
    if (active) return;
    active = new AbortController();
    const lifetime = AbortSignal.any([signal, active.signal]);
    const { target, emit } = selected;
    const connected = rpc;
    const publish = (value: boolean) => {
      if (!lifetime.aborted && nonRepo !== value) {
        nonRepo = value;
        emit(value);
      }
    };
    void Effect.runPromise(
      Stream.suspend(() =>
        session.vcsBranchStatus
          ? connected[WS_METHODS.subscribeVcsBranch]({ cwd: target.cwd })
          : connected[WS_METHODS.subscribeVcsStatus]({
              cwd: target.cwd,
              includeRemote: false,
            }).pipe(
              Stream.filter((event) => event._tag !== "remoteUpdated"),
              Stream.map(({ local }) => ({ refName: local.refName, isRepo: local.isRepo })),
            ),
      ).pipe(
        Stream.runForEach((status) =>
          Effect.gen(function* () {
            const isRepo =
              status.isRepo ??
              (status.refName !== null ||
                (yield* connected[WS_METHODS.vcsRefreshStatus]({
                  cwd: target.cwd,
                  localOnly: true,
                })).isRepo);
            publish(!isRepo);
          }),
        ),
        Effect.andThen(Effect.fail("Branch subscription ended")),
        Effect.ensuring(Effect.sync(() => publish(false))),
        Effect.retry(
          Schedule.min([Schedule.exponential("1 second"), Schedule.spaced("30 seconds")]),
        ),
      ),
      { signal: lifetime },
    ).catch(() => {});
  }

  return {
    reconcile,
    connect(next: Rpc | undefined) {
      stop();
      rpc = next;
      reconcile();
    },
    select(id: number, target: GitTarget | null, emit: (nonRepo: boolean) => void) {
      stop();
      selected = target ? { id, target, emit } : undefined;
      reconcile();
    },
    async initialize(id: number) {
      if (
        !rpc ||
        !active ||
        !selected ||
        selected.id !== id ||
        !session.canWriteGit ||
        !nonRepo ||
        initializing ||
        !isCurrent(selected.target)
      )
        return false;
      const owner = active;
      initializing = true;
      try {
        await Effect.runPromise(rpc[WS_METHODS.vcsInit]({ cwd: selected.target.cwd }), {
          signal: AbortSignal.any([signal, owner.signal]),
        });
        if (active === owner && nonRepo) {
          nonRepo = false;
          selected?.emit(false);
        }
        return true;
      } finally {
        if (active === owner) initializing = false;
      }
    },
  };
}
