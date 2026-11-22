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
import { msg } from "gt-i18n";
import { m } from "../i18n/catalog.ts";

const isProjectScript = Schema.is(ProjectScript);

function scriptIdentity(script: ProjectScript) {
  return JSON.stringify(script, Object.keys(script).sort());
}

export function retainedScriptKeys(
  shown: ReadonlyMap<string, ProjectScript>,
  scripts: readonly ProjectScript[],
) {
  const previous = Map.groupBy(
    [...shown].map(([key, script], index) => ({
      key,
      index,
      identity: scriptIdentity(script),
    })),
    (row) => row.identity,
  );
  const identities = scripts.map(scriptIdentity);
  const retained = new Map<number, string>();
  for (const [identity, indices] of Map.groupBy(identities.keys(), (i) => identities[i]!)) {
    const old = previous.get(identity);
    if (!old || old.length !== indices.length) continue;
    // Identical actions have no persistent ID that distinguishes one occurrence from another.
    if (indices.length > 1 && indices.some((index, i) => index !== old[i]!.index)) continue;
    indices.forEach((index, i) => retained.set(index, old[i]!.key));
  }
  return retained;
}

export function scriptSettingsPatch(
  settings: ServerSettings,
  project: { id: ProjectId; scripts: readonly ProjectScript[] },
  shown: ReadonlyMap<string, ProjectScript>,
  key: string | null,
  input: ProjectScriptInput | null,
) {
  const scripts = resolveProjectScripts(settings, project);
  const index =
    key === null
      ? undefined
      : [...retainedScriptKeys(shown, scripts)].find(([, retained]) => retained === key)?.[0];
  const original = index === undefined ? undefined : scripts[index];
  if ((!original && !input) || (key !== null && !original)) {
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
