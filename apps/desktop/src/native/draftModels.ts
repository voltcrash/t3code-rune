import {
  isProviderInstancePickerReady,
  providerInstanceEntriesFor,
  resolveProjectDefaultProviderModelSelection,
} from "@t3tools/client-runtime/state/provider-instances";
import {
  applyServerConfigProjection,
  type ServerConfigProjection,
} from "@t3tools/client-runtime/state/server-config-projection";
import {
  clampManagedRuntimeMode,
  ProjectId,
  RuntimeMode,
  WS_METHODS,
  type ModelSelection,
  type ServerConfig,
  type ServerSettings,
} from "@t3tools/contracts";
import { resolveNewThreadModelSelection } from "@t3tools/shared/usageLimits";
import { resolveProjectSettings } from "@t3tools/shared/projectSettings";
import * as NodeUtil from "node:util";
import * as Clock from "effect/Clock";
import * as Effect from "effect/Effect";
import * as Stream from "effect/Stream";
import * as Option from "effect/Option";
import * as Schedule from "effect/Schedule";
import {
  getProviderStatusDismissalKey,
  providerStatusPresentation,
} from "@t3tools/client-runtime/state/provider-status";
import { msg } from "gt-i18n";
import { m, onLocaleChange } from "../i18n/catalog.ts";
import { availableModels } from "./models.ts";
import { permissionOptions } from "./permissions.ts";
import type { openEnvironmentRpc } from "./environmentRpc.ts";
import type { projectSummary } from "./projects.ts";

export function draftModelCatalog(
  config: ServerConfig,
  projectId: ProjectId,
  root: string,
  settings: ServerSettings,
  now: number,
) {
  const { selection: preferred } = resolveNewThreadModelSelection({
    defaultSelection:
      settings.defaultModelSelection ??
      resolveProjectDefaultProviderModelSelection(config.providers, config.settings, projectId),
    fallbackSelection: settings.fallbackModelSelection,
    providers: config.providers,
    settings: config.settings,
    projectId,
    now,
  });
  const entries = providerInstanceEntriesFor(config.providers, config.settings, projectId);
  const permissions: Record<string, ReturnType<typeof permissionOptions>> = Object.create(null);
  const choices = entries.filter(isProviderInstancePickerReady).flatMap((entry) => {
    const supported = entry.snapshot.supportedRuntimeModes;
    const modes = RuntimeMode.literals.filter(
      (mode) => !supported?.length || supported.includes(mode),
    );
    const mode = modes.includes(settings.defaultRuntimeMode)
      ? settings.defaultRuntimeMode
      : modes[0];
    const runtime = mode && clampManagedRuntimeMode(config.managedPolicy, mode, modes);
    if (!runtime) return [];
    permissions[entry.instanceId] = permissionOptions(modes, config.managedPolicy);
    return availableModels(entry, settings, root).map((model) => {
      const isPreferred =
        preferred?.instanceId === entry.instanceId && preferred.model === model.slug;
      const selection: ModelSelection = isPreferred
        ? preferred
        : { instanceId: entry.instanceId, model: model.slug };
      return {
        id: JSON.stringify([entry.instanceId, model.slug]),
        name: model.name,
        provider: entry.displayName,
        runtime,
        selection,
        preferred: isPreferred,
      };
    });
  });
  return { choices, permissions };
}

export function draftModelReply(options: ReturnType<typeof draftModelCatalog>) {
  return {
    permissions: options.permissions,
    choices: options.choices.map(({ id, name, provider, selection, runtime, preferred }) => ({
      instance: selection.instanceId,
      id,
      name,
      provider,
      runtime,
      preferred,
    })),
  };
}

type Rpc = Effect.Success<ReturnType<typeof openEnvironmentRpc>>;
type ProjectRow = ReturnType<typeof projectSummary>;

type StatusTarget = { project: string; instance: string | null };

function statusBanner(
  config: ServerConfig,
  project: ProjectRow,
  environment: string,
  target: StatusTarget,
) {
  const id = ProjectId.make(project.id);
  const entries = providerInstanceEntriesFor(config.providers, config.settings, id);
  const instance =
    target.instance ??
    resolveProjectSettings(config.settings, id, project).settings.defaultModelSelection
      ?.instanceId ??
    resolveProjectDefaultProviderModelSelection(config.providers, config.settings, id)?.instanceId;
  const entry =
    entries.find((item) => item.instanceId === instance && item.enabled) ??
    (target.instance ? undefined : entries.find((item) => item.enabled));
  const status = entry?.snapshot;
  const key = status && getProviderStatusDismissalKey(environment, status);
  if (!key || !status) return null;
  const presentation = providerStatusPresentation(status, m);
  const provider = presentation.providerName;
  const dismissLabel =
    status.status === "error"
      ? msg("Dismiss {provider} provider error", { provider })
      : status.status === "warning"
        ? msg("Dismiss {provider} provider warning", { provider })
        : msg("Dismiss {provider} provider ready", { provider });
  return {
    key,
    instance: status.instanceId,
    ...presentation,
    dismissLabel: m(dismissLabel),
    refreshLabel: m(msg("Refresh {provider} provider status", { provider })),
    refresh: m(msg("Refresh")),
    refreshing: m(msg("Refreshing...")),
  };
}

export function watchDraftModels(
  signal: AbortSignal,
  projectById: (id: string) => ProjectRow | undefined,
  environment: string,
) {
  let rpc: Rpc | undefined;
  let selected:
    | { project: string; emit: (catalog: ReturnType<typeof draftModelReply>) => void }
    | undefined;
  let bannerTarget:
    | (StatusTarget & { emit: (banner: ReturnType<typeof statusBanner>) => void })
    | undefined;
  let banner: ReturnType<typeof statusBanner> | undefined;
  function publishBanner() {
    if (!bannerTarget) return;
    const project = projectById(bannerTarget.project);
    const next =
      project && !project.archived && Option.isSome(current)
        ? statusBanner(current.value.config, project, environment, bannerTarget)
        : null;
    if (!NodeUtil.isDeepStrictEqual(next, banner)) {
      banner = next;
      bannerTarget.emit(next);
    }
  }
  let current: Option.Option<ServerConfigProjection> = Option.none();
  let published: ReturnType<typeof draftModelReply> | undefined;
  let controller: AbortController | undefined;
  function publish() {
    if (!selected || Option.isNone(current)) return;
    const row = projectById(selected.project);
    if (!row || row.archived) return;
    const { config } = current.value;
    const projectId = ProjectId.make(row.id);
    const { settings } = resolveProjectSettings(config.settings, projectId, row);
    const next = draftModelReply(
      draftModelCatalog(
        config,
        projectId,
        row.path,
        settings,
        Effect.runSync(Clock.currentTimeMillis),
      ),
    );
    if (NodeUtil.isDeepStrictEqual(published, next)) return;
    published = next;
    selected.emit(next);
  }
  signal.addEventListener("abort", onLocaleChange(publishBanner), { once: true });
  function subscribe() {
    controller?.abort();
    controller = undefined;
    current = Option.none();
    published = undefined;
    publishBanner();
    if ((!selected && !bannerTarget) || !rpc) return;
    controller = new AbortController();
    const lifetime = AbortSignal.any([signal, controller.signal]);
    const connected = rpc;
    void Effect.runPromise(
      Stream.suspend(() =>
        connected[WS_METHODS.subscribeServerConfig]({
          providerStatusDeltas: true,
          ...(!selected && { workspaceSnapshotsOnDemand: true, providerCatalogsOnDemand: true }),
        }),
      ).pipe(
        Stream.runForEach((event) =>
          Effect.sync(() => {
            if (lifetime.aborted) return;
            const previous = Option.getOrUndefined(current)?.config;
            current = applyServerConfigProjection(current, event);
            const config = Option.getOrUndefined(current)?.config;
            if (
              previous?.providers !== config?.providers ||
              previous?.settings !== config?.settings ||
              previous?.managedPolicy !== config?.managedPolicy
            ) {
              publish();
              publishBanner();
            }
          }),
        ),
        Effect.andThen(Effect.fail("Draft model stream ended")),
        Effect.ensuring(
          Effect.sync(() => {
            if (!lifetime.aborted) {
              current = Option.none();
              publishBanner();
            }
          }),
        ),
        Effect.retry(
          Schedule.min([Schedule.exponential("1 second"), Schedule.spaced("30 seconds")]),
        ),
      ),
      { signal: lifetime },
    ).catch(() => {});
  }
  return {
    config: () => (selected ? Option.getOrUndefined(current)?.config : undefined),
    connect(next: Rpc | undefined) {
      rpc = next;
      subscribe();
    },
    select(project: string | null, emit: (catalog: ReturnType<typeof draftModelReply>) => void) {
      const wasSelected = selected !== undefined;
      selected = project === null ? undefined : { project, emit };
      if (wasSelected === Boolean(selected) && (selected || bannerTarget) && controller) {
        published = undefined;
        publish();
      } else subscribe();
    },
    selectBanner(
      target: StatusTarget | null,
      emit: (value: ReturnType<typeof statusBanner>) => void,
    ) {
      bannerTarget = target ? { ...target, emit } : undefined;
      banner = undefined;
      if ((selected || bannerTarget) && controller) publishBanner();
      else subscribe();
    },
    refreshProvider(key: string) {
      if (banner?.key !== key || !rpc) return undefined;
      return rpc[WS_METHODS.serverRefreshProviders]({
        instanceId: banner.instance,
        refreshModels: true,
        workspaceSnapshotsOnDemand: true,
        omitProviders: true,
      });
    },
    projectChanged(dirty: ReadonlySet<string>) {
      if (selected && dirty.has(selected.project)) publish();
      if (bannerTarget && dirty.has(bannerTarget.project)) publishBanner();
    },
  };
}
