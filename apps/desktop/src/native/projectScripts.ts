import { ProjectId, WS_METHODS, type ProjectScript, type ServerSettings } from "@t3tools/contracts";
import { primaryProjectScript, resolveProjectScripts } from "@t3tools/shared/projectScripts";
import type { ProjectScriptInput } from "@t3tools/shared/projectScripts";
import type { openEnvironmentRpc } from "./environmentRpc.ts";
import { scriptSettingsPatch } from "./scriptMutations.ts";
import * as Effect from "effect/Effect";
import * as NodeUtil from "node:util";

import { msg } from "gt-i18n";
import { m } from "../i18n/catalog.ts";

let nextScriptKey = 0;
type ScriptRow = {
  key: string;
  edit: string;
  run: string;

  primary: boolean;
  script: ProjectScript;
};

export function watchProjectScripts(
  projectScripts: (project: string) => readonly ProjectScript[] | undefined,
) {
  let settings: ServerSettings | undefined;
  let selected:
    | { project: string; emit: (rows: readonly ScriptRow[], available: boolean) => void }
    | undefined;
  const byKey = new Map<string, ProjectScript>();
  let previous: readonly ProjectScript[] | null | undefined;
  function publish() {
    if (!selected) return;
    const legacy = projectScripts(selected.project);
    const scripts =
      settings && legacy
        ? resolveProjectScripts(settings, {
            id: ProjectId.make(selected.project),
            scripts: legacy,
          })
        : null;
    if (NodeUtil.isDeepStrictEqual(previous, scripts)) return;
    previous = scripts;
    byKey.clear();
    const primary = primaryProjectScript(scripts ?? []);
    const rows = (scripts ?? []).map((script) => {
      const key = String(++nextScriptKey);
      byKey.set(key, script);
      return {
        key,
        script,
        edit: m(msg("Edit {name}", { name: script.name })),
        run: m(msg("Run {name}", { name: script.name })),

        primary: script === primary,
      };
    });
    selected.emit(rows, scripts !== null);
  }
  return {
    select(project: string, emit: NonNullable<typeof selected>["emit"]) {
      selected = { project, emit };
      previous = undefined;
      publish();
    },
    clear() {
      selected = undefined;
      byKey.clear();
    },
    configure(next: ServerSettings | undefined) {
      if (settings !== next) {
        settings = next;
        publish();
      }
    },
    publish,
    async save(
      project: string,
      key: string | null,
      input: ProjectScriptInput | null,
      activeRpc: () => Effect.Success<ReturnType<typeof openEnvironmentRpc>> | undefined,
      signal: AbortSignal,
    ) {
      const rpc = activeRpc();
      const target = selected;
      const original = key ? byKey.get(key) : undefined;
      if (!rpc || !settings || target?.project !== project || (key && !original)) {
        throw new Error(m(msg("Failed to save action.")));
      }
      const config = await Effect.runPromise(
        rpc[WS_METHODS.serverGetConfig]({}).pipe(Effect.timeout("20 seconds")),
        { signal },
      );
      const legacy = projectScripts(project);
      if (!legacy || !settings || activeRpc() !== rpc || target !== selected) {
        throw new Error(m(msg("Failed to save action.")));
      }
      const patch = scriptSettingsPatch(
        config.settings,
        config.environment.capabilities.projectSettingsOverrides === true,
        { id: ProjectId.make(project), scripts: legacy },
        original,
        input,
      );
      await Effect.runPromise(
        rpc[WS_METHODS.serverUpdateSettings]({ patch }).pipe(Effect.timeout("20 seconds")),
        { signal },
      );
    },
    lookup(project: string, key: string) {
      return selected?.project === project ? byKey.get(key) : undefined;
    },
  };
}
