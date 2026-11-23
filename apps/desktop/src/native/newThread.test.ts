import type { OrchestrationV2RuntimeRequest } from "@t3tools/contracts";
import * as DateTime from "effect/DateTime";
import { expect, it } from "vite-plus/test";
import { launchWindowStatus } from "./newThread.ts";

it("names a pending request ahead of run activity in a launch receipt", () => {
  const pending = (kind: OrchestrationV2RuntimeRequest["kind"], at: number) => ({
    kind,
    status: "pending" as const,
    createdAt: DateTime.makeUnsafe(at),
  });
  const question = pending("user_input", 2);
  const approval = pending("command", 1);
  expect(launchWindowStatus([question, approval], true)).toBe("approval");
  expect(launchWindowStatus([question, { ...approval, status: "resolved" }], false)).toBe("input");
  expect(launchWindowStatus([approval, pending("auth_refresh", 3)], true)).toBe("working");
  expect(launchWindowStatus([], false)).toBeNull();
});
