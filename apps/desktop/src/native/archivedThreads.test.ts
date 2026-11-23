import { ProjectId, ThreadId, type OrchestrationV2ArchivedShellSnapshot } from "@t3tools/contracts";
import * as DateTime from "effect/DateTime";
import { expect, it } from "vite-plus/test";
import { archivedThreadGroups } from "./archivedThreads.ts";

it("groups archived threads by project, newest archive first, without deleted ones", () => {
  const at = (iso: string) => DateTime.makeUnsafe(iso);
  const thread = (id: string, project: string, archived: string | null, deleted = false) => ({
    id: ThreadId.make(id),
    projectId: ProjectId.make(project),
    title: id,
    createdAt: at("2026-01-01T00:00:00Z"),
    archivedAt: archived ? at(archived) : null,
    deletedAt: deleted ? at("2026-01-03T00:00:00Z") : null,
  });
  const snapshot = {
    projects: [
      { id: ProjectId.make("p"), title: "Project" },
      { id: ProjectId.make("empty"), title: "Empty" },
    ],
    threads: [
      thread("old", "p", "2026-01-02T00:00:00Z"),
      thread("new", "p", "2026-01-05T00:00:00Z"),
      thread("gone", "p", "2026-01-06T00:00:00Z", true),
      thread("live", "empty", null),
    ],
  } as unknown as OrchestrationV2ArchivedShellSnapshot;
  expect(archivedThreadGroups(snapshot)).toEqual([
    {
      project: "p",
      title: "Project",
      threads: [
        { id: "new", title: "new", archived_at: 1767571200, created_at: 1767225600 },
        { id: "old", title: "old", archived_at: 1767312000, created_at: 1767225600 },
      ],
    },
  ]);
});
