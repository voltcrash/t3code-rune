import { ProjectId, type ProjectScript, type ServerSettings } from "@t3tools/contracts";
import { primaryProjectScript, resolveProjectScripts } from "@t3tools/shared/projectScripts";
import * as NodeUtil from "node:util";

import { msg } from "gt-i18n";
import { m } from "../i18n/catalog.ts";

let nextScriptKey = 0;
type ScriptRow = {
  key: string;
  name: string;
  run: string;
  icon: ProjectScript["icon"];
  primary: boolean;
};

export function watchProjectScripts(
  projectScripts: (project: string) => readonly ProjectScript[] | undefined,
) {
  let settings: ServerSettings | undefined;
  let selected: { project: string; emit: (rows: readonly ScriptRow[]) => void } | undefined;
  const byKey = new Map<string, ProjectScript>();
  let previous: readonly ProjectScript[] | undefined;
  function publish() {
    if (!selected) return;
    const legacy = projectScripts(selected.project);
    const scripts =
      settings && legacy
        ? resolveProjectScripts(settings, {
            id: ProjectId.make(selected.project),
            scripts: legacy,
          })
        : [];
    if (NodeUtil.isDeepStrictEqual(previous, scripts)) return;
    previous = scripts;
    byKey.clear();
    const primary = primaryProjectScript(scripts);
    const rows = scripts.map((script) => {
      const key = String(++nextScriptKey);
      byKey.set(key, script);
      return {
        key,
        name: script.name,
        run: m(msg("Run {name}", { name: script.name })),
        icon: script.icon,
        primary: script === primary,
      };
    });
    selected.emit(rows);
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
    lookup(project: string, key: string) {
      return selected?.project === project ? byKey.get(key) : undefined;
    },
  };
}
