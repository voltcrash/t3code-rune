import { bootstrapRemoteBearerSession } from "@t3tools/client-runtime/authorization";
import { fetchRemoteEnvironmentDescriptor } from "@t3tools/client-runtime/environment";
import {
  layerRemoteHttpClient,
  makeEnvironmentHttpApiGroupClient,
} from "@t3tools/client-runtime/rpc";
import { AuthOrchestrationReadScope } from "@t3tools/contracts";
import {
  readPairingAppLink,
  resolveRemotePairingTarget,
  type ResolvedRemotePairingTarget,
} from "@t3tools/shared/remote";
import * as Effect from "effect/Effect";
import { FetchHttpClient } from "effect/http";
import * as Layer from "effect/Layer";

const remoteHttp = layerRemoteHttpClient(globalThis.fetch);
const localHttp = Layer.merge(
  remoteHttp,
  Layer.succeed(FetchHttpClient.RequestInit, { redirect: "error" }),
);

const pair = Effect.fn("nativeDesktop.pair")(function* (
  target: Pick<ResolvedRemotePairingTarget, "credential" | "httpBaseUrl">,
) {
  const descriptor = yield* fetchRemoteEnvironmentDescriptor(target);
  const session = yield* bootstrapRemoteBearerSession({
    ...target,
    scopes: [AuthOrchestrationReadScope],
    clientMetadata: { label: "T3 Code Desktop", deviceType: "desktop" },
  });
  return { label: descriptor.label, httpBaseUrl: target.httpBaseUrl, token: session.access_token };
});

export async function pairWithTarget(
  target: Pick<ResolvedRemotePairingTarget, "credential" | "httpBaseUrl">,
  signal: AbortSignal,
  http: typeof remoteHttp = localHttp,
) {
  const session = await Effect.runPromise(pair(target).pipe(Effect.provide(http)), { signal });
  return { ...session, http, signal };
}

export function pairWithEnvironment(link: string, signal: AbortSignal) {
  const target = resolveRemotePairingTarget({ pairingUrl: readPairingAppLink(link) ?? link });
  return pairWithTarget(target, signal, remoteHttp);
}

export const readProjects = (session: Awaited<ReturnType<typeof pairWithEnvironment>>) =>
  Effect.runPromise(
    Effect.gen(function* () {
      const client = yield* makeEnvironmentHttpApiGroupClient(session.httpBaseUrl, "projects");
      const snapshot = yield* client.snapshot({
        headers: { authorization: `Bearer ${session.token}` },
      });
      return snapshot.projects
        .filter((project) => project.deletedAt === null)
        .map((project) => ({
          title: project.title,
          path: project.workspaceRoot,
          archived: project.archivedAt != null,
        }));
    }).pipe(Effect.timeout("20 seconds"), Effect.provide(session.http)),
    { signal: session.signal },
  );
