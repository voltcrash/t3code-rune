import * as NodeWS from "@effect/platform-node/NodeWS";
import { resolveRemoteWebSocketConnectionUrl } from "@t3tools/client-runtime/authorization";
import { ORCHESTRATION_PROTOCOL_VERSION, WsRpcGroup } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Schedule from "effect/Schedule";
import { RpcClient, RpcSerialization } from "effect/rpc";
import * as Socket from "effect/socket/Socket";

export const openEnvironmentRpc = Effect.fn("nativeDesktop.openEnvironmentRpc")(
  function* (session: { readonly httpBaseUrl: string; readonly token: string }) {
    const ws = new URL("/ws", session.httpBaseUrl);
    ws.protocol = ws.protocol === "https:" ? "wss:" : "ws:";
    ws.searchParams.set("orchestrationProtocol", String(ORCHESTRATION_PROTOCOL_VERSION));
    const url = yield* resolveRemoteWebSocketConnectionUrl({
      wsBaseUrl: ws.href,
      httpBaseUrl: session.httpBaseUrl,
      bearerToken: session.token,
      clientMetadata: { label: "T3 Code Desktop", deviceType: "desktop" },
    });
    const protocol = yield* Layer.build(
      RpcClient.layerProtocolSocket({
        retryTransientErrors: false,
        retryPolicy: Schedule.recurs(0),
      }).pipe(
        Layer.provide(Socket.layerWebSocket(url)),
        Layer.provide(RpcSerialization.layerJson),
        Layer.provide(NodeWS.layerWebSocketConstructor),
      ),
    );
    return yield* RpcClient.make(WsRpcGroup).pipe(Effect.provideContext(protocol));
  },
);
