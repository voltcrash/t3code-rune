import { expect, it } from "@effect/vitest";
import {
  ProviderDriverKind,
  ProviderInstanceId,
  WS_METHODS,
  type ServerProvider,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import { composerSkillSearch } from "./composerSkills.ts";

const checkedAt = "2026-01-01T00:00:00.000Z";
const cwd = "/project";
function provider(driver: string): ServerProvider {
  return {
    instanceId: ProviderInstanceId.make(driver),
    driver: ProviderDriverKind.make(driver),
    enabled: true,
    installed: true,
    version: null,
    status: "ready",
    auth: { status: "authenticated" },
    checkedAt,
    models: [],
    slashCommands: [],
    skills: [{ name: "machine-skill", path: "/skills/machine", enabled: true }],
  };
}

it.effect("keeps machine skills when the provider has no workspace scanner", () =>
  Effect.gen(function* () {
    const held = provider("acp-registry");
    const rpc = {
      [WS_METHODS.serverRefreshProviders]: () =>
        Effect.succeed({ providers: [{ ...held, skills: [], workspaceSnapshots: [] }] }),
    };
    const search = composerSkillSearch();
    const scope = yield* Effect.scope;
    const entries = yield* search(rpc, scope, cwd, held.instanceId, held, "");
    expect(entries.map((entry) => entry.link)).toEqual(["$machine-skill"]);
    const slash = { model: "Switch response model", showSkills: true };
    const results = yield* search(rpc, scope, cwd, held.instanceId, held, "", slash);
    expect(results.map((entry) => entry.path)).toEqual(["/model", "machine-skill"]);
    const exact = yield* search(rpc, scope, cwd, held.instanceId, held, "skill:machine", slash);
    expect(exact.map((entry) => entry.link)).toEqual(["$machine-skill"]);
    const disabled = yield* search(rpc, scope, cwd, held.instanceId, held, "", {
      ...slash,
      showSkills: false,
    });
    expect(disabled.map((entry) => entry.path)).toEqual(["/model"]);
  }).pipe(Effect.scoped),
);

it.effect("reads the rescanned checkout when a session changes its skills", () =>
  Effect.gen(function* () {
    const held = provider("claudeAgent");
    const search = composerSkillSearch();
    const scope = yield* Effect.scope;
    let name = "before";
    const rpc = {
      [WS_METHODS.serverRefreshProviders]: () =>
        Effect.succeed({
          providers: [
            {
              ...held,
              workspaceSnapshots: [
                {
                  cwd,
                  checkedAt,
                  slashCommands: [],
                  skills: [{ name, path: "/skills/" + name, enabled: true }],
                },
              ],
            },
          ],
        }),
    };
    expect((yield* search(rpc, scope, cwd, held.instanceId, held, ""))[0]?.path).toBe("before");
    name = "after";
    const updated = { ...held, workspaceRescannedAt: "2026-01-01T01:00:00.000Z" };
    expect((yield* search(rpc, scope, cwd, held.instanceId, updated, ""))[0]?.path).toBe("after");
  }).pipe(Effect.scoped),
);
