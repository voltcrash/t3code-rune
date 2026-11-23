import type { OrchestrationV2ArchivedShellSnapshot } from "@t3tools/contracts";
import * as DateTime from "effect/DateTime";

const seconds = (at: DateTime.Utc) => Math.floor(DateTime.toEpochMillis(at) / 1000);

/** Archived threads grouped under their projects, newest archive first, as the web Archive page lists them. */
export function archivedThreadGroups(snapshot: OrchestrationV2ArchivedShellSnapshot) {
  return snapshot.projects.flatMap((project) => {
    const threads = snapshot.threads
      .filter(
        (thread) =>
          thread.projectId === project.id && thread.archivedAt !== null && !thread.deletedAt,
      )
      .map((thread) => ({
        id: thread.id,
        title: thread.title,
        archived_at: seconds(thread.archivedAt ?? thread.createdAt),
        created_at: seconds(thread.createdAt),
      }))
      .toSorted((a, b) => b.archived_at - a.archived_at || b.id.localeCompare(a.id));
    return threads.length ? [{ project: project.id, title: project.title, threads }] : [];
  });
}
