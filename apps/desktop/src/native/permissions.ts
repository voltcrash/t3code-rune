import {
  managedRuntimeModeDenial,
  runtimeModeMessages,
} from "@t3tools/client-runtime/contract-labels";
import { RuntimeMode, WS_METHODS, type ManagedPolicySummary } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as Schema from "effect/Schema";
import { mFallback } from "gt-i18n";
import type { openEnvironmentRpc } from "./environmentRpc.ts";

type EnvironmentRpc = Effect.Success<ReturnType<typeof openEnvironmentRpc>>;

export class PermissionUnavailable extends Schema.TaggedError<PermissionUnavailable>()(
  "PermissionUnavailable",
  { message: Schema.String },
) {}

export const permissionLabel = (mode: RuntimeMode) => mFallback(runtimeModeMessages[mode].label);

export function permissionOptions(
  modes: readonly RuntimeMode[],
  policy: ManagedPolicySummary | undefined,
) {
  return modes.map((mode) => {
    const denied = managedRuntimeModeDenial(policy, mode);
    return {
      mode,
      label: permissionLabel(mode),
      description: mFallback(runtimeModeMessages[mode].description),
      ...(denied ? { denied } : {}),
    };
  });
}

export const readThreadPermissions = Effect.fn("nativeDesktop.readThreadPermissions")(function* (
  rpc: EnvironmentRpc,
  instance: string,
) {
  const config = yield* rpc[WS_METHODS.serverGetConfig]({});
  const provider = config.providers.find((p) => p.instanceId === instance);
  if (!provider)
    return yield* new PermissionUnavailable({
      message: "This thread’s provider is unavailable on this environment.",
    });
  const modes = RuntimeMode.literals.filter(
    (mode) =>
      !provider.supportedRuntimeModes?.length || provider.supportedRuntimeModes.includes(mode),
  );
  return permissionOptions(modes, config.managedPolicy);
});
