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

const http = layerRemoteHttpClient(globalThis.fetch);

const pair = Effect.fn("nativeDesktop.pair")(function* (target: ResolvedRemotePairingTarget) {
  const descriptor = yield* fetchRemoteEnvironmentDescriptor(target);
  const session = yield* bootstrapRemoteBearerSession({
    ...target,
    scopes: [AuthOrchestrationReadScope],
    clientMetadata: { label: "T3 Code Desktop", deviceType: "desktop" },
  });
  return { label: descriptor.label, httpBaseUrl: target.httpBaseUrl, token: session.access_token };
});

export function pairWithEnvironment(link: string) {
  const target = resolveRemotePairingTarget({ pairingUrl: readPairingAppLink(link) ?? link });
  return Effect.runPromise(pair(target).pipe(Effect.provide(http)));
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
    }).pipe(Effect.timeout("20 seconds"), Effect.provide(http)),
  );
