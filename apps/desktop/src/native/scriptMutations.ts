import {
  ProjectId,
  ProjectScript,
  type ResolvedKeybindingsConfig,
  type ServerSettings,
} from "@t3tools/contracts";
import {
  buildProjectScript,
  commandForProjectScript,
  nextProjectScriptId,
  releaseClaimedRoles,
  resolveProjectScripts,
  type ProjectScriptInput,
} from "@t3tools/shared/projectScripts";
import { keybindingValueForCommand } from "@t3tools/shared/keybindings";
import * as Schema from "effect/Schema";
import * as NodeUtil from "node:util";
import { msg } from "gt-i18n";
import { m } from "../i18n/catalog.ts";

const isProjectScript = Schema.is(ProjectScript);

export function scriptSettingsPatch(
  settings: ServerSettings,
  project: { id: ProjectId; scripts: readonly ProjectScript[] },
  original: ProjectScript | undefined,
  input: ProjectScriptInput | null,
  occurrence = 0,
) {
  const scripts = resolveProjectScripts(settings, project);
  const index = original
    ? scripts.findIndex((s) => NodeUtil.isDeepStrictEqual(s, original) && occurrence-- === 0)
    : -1;
  if ((!original && !input) || (original && index < 0)) {
    throw new Error(m(msg("Script not found.")));
  }
  let updated: readonly ProjectScript[];
  if (input) {
    const name = input.name.trim();
    const command = input.command.trim();
    if (!name) throw new Error(m(msg("Name is required.")));
    if (!command) throw new Error(m(msg("Command is required.")));
    const id =
      original?.id ??
      nextProjectScriptId(
        name,
        scripts.map((s) => s.id),
      );
    const script = buildProjectScript(id, {
      ...input,
      name,
      command,
      previewUrl: input.previewUrl?.trim() || null,
    });
    if (!isProjectScript(script)) throw new Error(m(msg("Failed to save action.")));
    updated = scripts.map((s, i) => (i === index ? script : releaseClaimedRoles(s, input)));
    if (!original) updated = [...updated, script];
  } else {
    updated = scripts.filter((_, i) => i !== index);
  }
  // The server merges this field into the current project override under its settings lock.
  return { projectScriptOverrides: { [project.id]: updated } };
}

export function obsoleteScriptShortcuts(
  settings: ServerSettings,
  legacy: ReadonlyMap<string, readonly ProjectScript[]>,
  bindings: ResolvedKeybindingsConfig,
  id: string,
) {
  const command = commandForProjectScript(id);
  if (!command || !bindings.some((rule) => rule.command === command && !rule.whenAst)) return [];
  const usesId = (scripts: readonly ProjectScript[]) => scripts.some((s) => s.id === id);
  if (
    usesId(settings.defaultProjectScripts) ||
    Object.values(settings.projectSettingsOverrides).some((entry) =>
      usesId(entry.defaultProjectScripts ?? []),
    )
  )
    return [];
  if (!settings.projectSettingsFolded) {
    const ids = new Set([...legacy.keys(), ...Object.keys(settings.projectScriptOverrides)]);
    for (const project of ids) {
      if (
        usesId(
          resolveProjectScripts(settings, {
            id: ProjectId.make(project),
            scripts: legacy.get(project) ?? [],
          }),
        )
      )
        return [];
    }
  }
  return bindings.flatMap((rule) => {
    if (rule.command !== command || rule.whenAst) return [];
    const key = keybindingValueForCommand([rule], command);
    return key ? [{ key, command }] : [];
  });
}
