import { ProjectId, ProjectScript, type ServerSettings } from "@t3tools/contracts";
import {
  buildProjectScript,
  nextProjectScriptId,
  releaseClaimedRoles,
  resolveProjectScripts,
  type ProjectScriptInput,
} from "@t3tools/shared/projectScripts";
import * as Schema from "effect/Schema";
import * as NodeUtil from "node:util";
import { msg } from "gt-i18n";
import { m } from "../i18n/catalog.ts";

const isProjectScript = Schema.is(ProjectScript);

export function scriptSettingsPatch(
  settings: ServerSettings,
  supportsOverrides: boolean,
  project: { id: ProjectId; scripts: readonly ProjectScript[] },
  original: ProjectScript | undefined,
  input: ProjectScriptInput | null,
) {
  const scripts = resolveProjectScripts(settings, project);
  if (
    (!original && !input) ||
    (original && !scripts.some((s) => NodeUtil.isDeepStrictEqual(s, original)))
  ) {
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
    updated = scripts.map((s) => (s.id === original?.id ? script : releaseClaimedRoles(s, input)));
    if (!original) updated = [...updated, script];
  } else {
    updated = scripts.filter((s) => s.id !== original?.id);
  }
  return supportsOverrides
    ? {
        projectSettingsOverrides: {
          [project.id]: {
            ...settings.projectSettingsOverrides[project.id],
            defaultProjectScripts: updated,
          },
        },
      }
    : { projectScriptOverrides: { [project.id]: updated } };
}
