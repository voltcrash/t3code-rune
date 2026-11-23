import { providerWorkspaceSnapshotBasis } from "@t3tools/client-runtime/state/server-config-projection";
import * as Equal from "effect/Equal";
import { resolveProviderSkillsForCwd } from "@t3tools/client-runtime/providerSkills";
import { searchProviderSkills } from "@t3tools/client-runtime/providerSkillSearch";
import {
  isProviderWorkspaceSnapshotCurrent,
  isProviderWorkspaceSnapshotPending,
  PROVIDER_WORKSPACE_SNAPSHOT_TTL_MS,
  ProviderInstanceId,
  WS_METHODS,
  type ServerProvider,
} from "@t3tools/contracts";
import { formatProviderSkillDisplayName } from "@t3tools/shared/inlineSkills";
import * as Fiber from "effect/Fiber";
import type * as Scope from "effect/Scope";
import * as Clock from "effect/Clock";
import * as Effect from "effect/Effect";
import type { openEnvironmentRpc } from "./environmentRpc.ts";

type Refresh = Effect.Success<
  ReturnType<typeof openEnvironmentRpc>
>[typeof WS_METHODS.serverRefreshProviders];
type Rpc = {
  [WS_METHODS.serverRefreshProviders]: (
    input: Parameters<Refresh>[0],
  ) => Effect.Effect<
    { readonly providers: ReadonlyArray<ServerProvider> },
    Effect.Error<ReturnType<Refresh>>
  >;
};
export function composerSkillSearch() {
  let scan:
    | {
        rpc: Rpc;
        cwd: string;
        instance: string;
        basis: ReturnType<typeof providerWorkspaceSnapshotBasis>;
        until: number;
        provider: Fiber.Fiber<ServerProvider | undefined>;
      }
    | undefined;
  return Effect.fn("nativeDesktop.searchComposerSkills")(function* (
    rpc: Rpc,
    scope: Scope.Scope,
    cwd: string,
    instance: string,
    held: ServerProvider | undefined,
    query: string,
  ) {
    const now = yield* Clock.currentTimeMillis;
    let provider = held;
    const snapshot = held?.workspaceSnapshots?.find((value) => value.cwd === cwd);
    if (
      !snapshot ||
      isProviderWorkspaceSnapshotPending(snapshot) ||
      !isProviderWorkspaceSnapshotCurrent(snapshot, now)
    ) {
      const basis = providerWorkspaceSnapshotBasis(held);
      if (
        !scan ||
        scan.rpc !== rpc ||
        scan.cwd !== cwd ||
        scan.instance !== instance ||
        !Equal.equals(scan.basis, basis) ||
        scan.until <= now
      ) {
        const request = rpc[WS_METHODS.serverRefreshProviders]({
          instanceId: ProviderInstanceId.make(instance),
          cwd,
          workspaceSnapshotsOnDemand: true,
          providerCatalogsOnDemand: true,
          machineCatalog: true,
        });
        const provider = yield* request.pipe(
          Effect.timeout("12 seconds"),
          Effect.map(({ providers }) => providers.find((p) => p.instanceId === instance)),
          Effect.orElseSucceed(() => undefined),
          Effect.forkIn(scope),
        );
        scan = {
          rpc,
          cwd,
          instance,
          basis,
          until: now + PROVIDER_WORKSPACE_SNAPSHOT_TTL_MS,
          provider,
        };
      }
      const reading = scan;
      const result = yield* Fiber.join(reading.provider);
      const updated = result?.workspaceSnapshots?.find((s) => s.cwd === cwd);
      if (!updated || isProviderWorkspaceSnapshotPending(updated)) {
        reading.until = Math.min(reading.until, now + 10_000);
      }
      provider = updated ? result : (held ?? result);
    }
    return searchProviderSkills(
      provider ? resolveProviderSkillsForCwd(provider, cwd) : [],
      query,
      80,
    ).map((skill) => ({
      path: skill.name,
      kind: "skill",
      link: "$" + skill.name,
      label: formatProviderSkillDisplayName(skill),
      description: skill.shortDescription ?? skill.description ?? "",
    }));
  });
}
