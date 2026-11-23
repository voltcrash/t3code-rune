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

type Rpc = Effect.Success<ReturnType<typeof openEnvironmentRpc>>;
export function composerSkillSearch() {
  let scan:
    | {
        rpc: Rpc;
        cwd: string;
        instance: string;
        digest: string | undefined;
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
      if (
        !scan ||
        scan.rpc !== rpc ||
        scan.cwd !== cwd ||
        scan.instance !== instance ||
        scan.digest !== held?.catalogDigest ||
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
          digest: held?.catalogDigest,
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
      provider = result ?? held;
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
