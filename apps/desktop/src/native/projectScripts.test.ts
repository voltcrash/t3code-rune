import { DEFAULT_SERVER_SETTINGS, type ProjectScript } from "@t3tools/contracts";
import { expect, it } from "vite-plus/test";
import { watchProjectScripts } from "./projectScripts.ts";

it("invalidates displayed actions when their command, project, or environment config changes", () => {
  const action: ProjectScript = {
    id: "test",
    name: "Check",
    command: "echo one",
    icon: "test",
    runOnWorktreeCreate: false,
  };
  const scripts = watchProjectScripts((id) => (id === "p" ? [action] : undefined));
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
      p: { defaultProjectScripts: [{ ...action, command: "echo two" }] },
    },
  };
  scripts.configure(settings);
  expect(scripts.lookup("p", original)).toBeUndefined();
  expect(scripts.lookup("p", key!)?.command).toBe("echo two");
  const updated = key;
  scripts.configure({ ...settings });
  expect(key).toBe(updated);
  scripts.configure(undefined);
  expect(scripts.lookup("p", updated!)).toBeUndefined();
  expect(key).toBeUndefined();
  scripts.configure(settings);
  scripts.select("other", receive);
  expect(key).toBeUndefined();
});
