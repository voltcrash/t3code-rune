import { applyServerSettingsPatch } from "@t3tools/shared/serverSettings";
import { DEFAULT_SERVER_SETTINGS, ProjectId, type ProjectScript } from "@t3tools/contracts";
import { expect, it } from "vite-plus/test";
import { obsoleteScriptShortcuts, scriptSettingsPatch } from "./scriptMutations.ts";
import { watchProjectScripts } from "./projectScripts.ts";

it("invalidates displayed actions when their command, project, or environment config changes", () => {
  const action: ProjectScript = {
    id: "test",
    name: "Check",
    command: "echo one",
    icon: "test",
    runOnWorktreeCreate: false,
  };
  const scripts = watchProjectScripts(
    (id) => (id === "p" ? [action] : undefined),
    () => new Map(),
  );
  let key: string | undefined;
  const receive = (rows: readonly { key: string }[]) => {
    key = rows[0]?.key;
  };
  scripts.select("p", receive);
  expect(key).toBeUndefined();
  scripts.configure({ ...DEFAULT_SERVER_SETTINGS, projectSettingsFolded: false });
  const original = key!;
  expect(scripts.lookup("p", original)?.command).toBe("echo one");
  expect(scripts.lookup("other", original)).toBeUndefined();
  const settings = {
    ...DEFAULT_SERVER_SETTINGS,
    projectSettingsOverrides: {
      [ProjectId.make("p")]: { defaultProjectScripts: [{ ...action, command: "echo two" }] },
    },
  };
  scripts.configure(settings);
  expect(scripts.lookup("p", original)).toBeUndefined();
  expect(scripts.lookup("p", key!)?.command).toBe("echo two");
  const updated = key;
  scripts.configure({
    ...settings,
    projectSettingsOverrides: {
      [ProjectId.make("p")]: {
        defaultProjectScripts: [
          { ...action, command: "echo two" },
          { ...action, id: "other" },
        ],
      },
    },
  });
  expect(key).toBe(updated);
  scripts.configure({ ...settings });
  expect(key).toBe(updated);
  scripts.configure(undefined);
  expect(scripts.lookup("p", updated!)).toBeUndefined();
  expect(key).toBeUndefined();
  scripts.configure(settings);
  scripts.select("other", receive);
  expect(key).toBeUndefined();
});

it("edits the project's effective scripts without replacing other project settings", () => {
  const existing = {
    id: "setup",
    name: "Setup",
    icon: "build",
    command: "old",
    runOnWorktreeCreate: true,
  } as const;
  const project = { id: ProjectId.make("p"), scripts: [] };
  const settings = { ...DEFAULT_SERVER_SETTINGS, defaultProjectScripts: [existing] };
  const input = {
    name: " Setup ",
    command: " new ",
    icon: "test",
    runOnWorktreeCreate: true,
    waitForSetup: true,
    runOnSettle: false,
    stopOnSettle: true,
    previewUrl: null,
    autoOpenPreview: false,
  } as const;
  const patch = (original: ProjectScript | undefined, form: typeof input | null) =>
    scriptSettingsPatch(settings, project, original, form);
  const scripts = (p: ReturnType<typeof patch>) => p.projectScriptOverrides.p;
  expect(scripts(patch(undefined, input))).toMatchObject([
    { id: "setup", runOnWorktreeCreate: false },
    { id: "setup-2", command: "new", async: false, stopOnSettle: true },
  ]);
  expect(scripts(patch(existing, input))).toMatchObject([{ id: "setup", command: "new" }]);
  expect(scripts(patch(existing, null))).toEqual([]);
  expect(() => patch({ ...existing, command: "replaced" }, null)).toThrow();
  expect(settings.defaultProjectScripts).toEqual([existing]);
  const concurrent = {
    ...settings,
    projectSettingsOverrides: { p: { newWorktreesStartFromOrigin: true } },
  };
  expect(
    applyServerSettingsPatch(concurrent, patch(existing, null)).projectSettingsOverrides.p,
  ).toEqual({ newWorktreesStartFromOrigin: true, defaultProjectScripts: [] });
  const binding = {
    command: "script.setup.run",
    shortcut: {
      key: "b",
      modKey: true,
      ctrlKey: false,
      altKey: false,
      metaKey: false,
      shiftKey: false,
    },
  } as const;
  const bindings = [
    binding,
    { ...binding, whenAst: { type: "identifier", name: "terminalFocus" } },
  ] as const;
  expect(obsoleteScriptShortcuts(settings, new Map(), bindings, "setup")).toEqual([]);
  const cleared = { ...settings, defaultProjectScripts: [], projectSettingsFolded: false };
  expect(
    obsoleteScriptShortcuts(cleared, new Map([["other", [existing]]]), bindings, "setup"),
  ).toEqual([]);
  expect(obsoleteScriptShortcuts(cleared, new Map(), bindings, "setup")).toEqual([
    { key: "mod+b", command: "script.setup.run" },
  ]);
});
