import { applyServerSettingsPatch } from "@t3tools/shared/serverSettings";
import {
  DEFAULT_SERVER_SETTINGS,
  ProjectId,
  WS_METHODS,
  type ProjectScript,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
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
  let keys: string[] = [];
  let key: string | undefined;
  const receive = (rows: readonly { key: string }[]) => {
    keys = rows.map((row) => row.key);
    key = keys[0];
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
  const duplicates = [action, { ...action, command: "different" }, { ...action }];
  const setRows = (rows: ProjectScript[]) =>
    scripts.configure({
      ...settings,
      projectSettingsOverrides: { [ProjectId.make("p")]: { defaultProjectScripts: rows } },
    });
  setRows(duplicates);
  const priorKeys = [...keys];
  setRows([...duplicates, { ...action, id: "another" }]);
  expect(keys.slice(0, 3)).toEqual(priorKeys);
  expect(new Set(keys).size).toBe(keys.length);
  scripts.configure(undefined);
  expect(scripts.lookup("p", updated!)).toBeUndefined();
  expect(key).toBeUndefined();
  scripts.configure(settings);
  scripts.select("other", receive);
  expect(key).toBeUndefined();
});

it.each([false, true])(
  "rejects deletion when an identical action is edited, with replacement appended: %s",
  async (append) => {
    const action: ProjectScript = {
      id: "build",
      name: "Build",
      command: "build",
      icon: "build",
      runOnWorktreeCreate: false,
    };
    const other = { ...action, id: "lint", command: "lint" };
    const initial = { ...DEFAULT_SERVER_SETTINGS, defaultProjectScripts: [action, other, action] };
    const fresh = {
      ...initial,
      defaultProjectScripts: [
        { ...action, command: "changed" },
        other,
        action,
        ...(append ? [action] : []),
      ],
    };
    const scripts = watchProjectScripts(
      () => [],
      () => new Map(),
    );
    let keys: string[] = [];
    scripts.select("p", (rows) => {
      keys = rows.map((row) => row.key);
    });
    scripts.configure(initial);
    const [first, middle, last] = keys;
    let writes = 0;
    const rpc = {
      [WS_METHODS.serverGetConfig]: () => Effect.succeed({ settings: fresh, keybindings: [] }),
      [WS_METHODS.serverUpdateSettings]: () => {
        writes++;
        return Effect.succeed(fresh);
      },
    } as unknown as NonNullable<ReturnType<Parameters<typeof scripts.save>[3]>>;
    const remove = () => scripts.save("p", first!, null, () => rpc, new AbortController().signal);
    await expect(remove()).rejects.toThrow();
    scripts.configure(fresh);
    expect(keys[1]).toBe(middle);
    expect(keys).not.toContain(first);
    expect(keys).not.toContain(last);
    await expect(remove()).rejects.toThrow();
    expect(writes).toBe(0);
  },
);

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
    scriptSettingsPatch(
      settings,
      project,
      original ? new Map([["selected", original]]) : new Map(),
      original ? "selected" : null,
      form,
    );
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
    projectSettingsOverrides: { [project.id]: { newWorktreesStartFromOrigin: true } },
  };
  expect(
    applyServerSettingsPatch(concurrent, patch(existing, null)).projectSettingsOverrides[
      project.id
    ],
  ).toEqual({ newWorktreesStartFromOrigin: true, defaultProjectScripts: [] });
  const duplicate = { ...existing, command: "another", runOnWorktreeCreate: false };
  const sameIds = { ...settings, defaultProjectScripts: [existing, duplicate, { ...existing }] };
  const shown = new Map(sameIds.defaultProjectScripts.map((s, index) => [String(index), s]));
  expect(scripts(scriptSettingsPatch(sameIds, project, shown, "1", null))).toEqual([
    existing,
    existing,
  ]);
  expect(
    scripts(scriptSettingsPatch(sameIds, project, shown, "2", input))?.map((s) => s.command),
  ).toEqual(["old", "another", "new"]);
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
