import { T3_PROJECT_FILE_NAME, WS_METHODS, type T3ProjectFileScript } from "@t3tools/contracts";
import { parseT3ProjectFile } from "@t3tools/shared/t3ProjectFile";
import * as Effect from "effect/Effect";
import type { openEnvironmentRpc } from "./environmentRpc.ts";

export const readProjectFile = Effect.fn("nativeDesktop.readProjectFile")(
  function* (rpc: Effect.Success<ReturnType<typeof openEnvironmentRpc>>, cwd: string) {
    const result = yield* rpc[WS_METHODS.projectsReadFile]({
      cwd,
      relativePath: T3_PROJECT_FILE_NAME,
      allowMissing: true,
    });
    return result.exists === false || result.truncated ? null : parseT3ProjectFile(result.contents);
  },
  Effect.orElseSucceed(() => null),
);

type Rpc = Effect.Success<ReturnType<typeof openEnvironmentRpc>>;

export function watchProjectFileActions(
  signal: AbortSignal,
  location: () => { rpc: Rpc; cwd: string } | undefined,
  publish: (scripts: readonly T3ProjectFileScript[]) => void,
) {
  let active: { rpc: Rpc; cwd: string; controller: AbortController } | undefined;
  async function read(owner: NonNullable<typeof active>) {
    const lifetime = AbortSignal.any([signal, owner.controller.signal]);
    const file = await Effect.runPromise(
      readProjectFile(owner.rpc, owner.cwd).pipe(Effect.timeout("20 seconds")),
      { signal: lifetime },
    );
    return active === owner && !lifetime.aborted ? (file?.scripts ?? []) : [];
  }
  return {
    reconcile() {
      const next = location();
      if (active?.rpc === next?.rpc && active?.cwd === next?.cwd) return;
      active?.controller.abort();
      active = undefined;
      publish([]);
      if (!next || signal.aborted) return;
      const owner = { ...next, controller: new AbortController() };
      active = owner;
      void read(owner)
        .then((scripts) => {
          if (owner === active && !signal.aborted) publish(scripts);
        })
        .catch(() => {});
    },
    async current() {
      const owner = active;
      if (!owner) return [];
      const scripts = await read(owner);
      const latest = location();
      if (owner !== active || latest?.cwd !== owner.cwd || latest.rpc !== owner.rpc) return [];
      publish(scripts);
      return scripts;
    },
  };
}
