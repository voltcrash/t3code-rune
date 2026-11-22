import type * as NodeSqlite from "node:sqlite";
import { msg } from "gt-i18n";
import { m } from "../i18n/catalog.ts";

export function threadDetailsLabels() {
  return {
    panel: m(msg("Thread details")),
    toggle: m(msg("Toggle thread details panel")),
    context: m(msg("Run context: {context}", { context: "{context}" })),
    checkout: m(msg("Current checkout")),
    locked: m(msg("Local checkout")),
    worktree: m(msg("New worktree")),
    copyPath: m(msg("Copy full path")),
  };
}

function withLayout<T>(
  open: () => NodeSqlite.DatabaseSync,
  run: (db: NodeSqlite.DatabaseSync) => T,
): T {
  const db = open();
  try {
    db.exec(
      "CREATE TABLE IF NOT EXISTS hidden_details (environment TEXT, draft INTEGER, target TEXT, PRIMARY KEY(environment, draft, target))",
    );
    return run(db);
  } finally {
    db.close();
  }
}

export function hiddenThreadDetails(open: () => NodeSqlite.DatabaseSync, environment: string) {
  return withLayout(open, (db) => {
    const threads: string[] = [];
    const drafts: string[] = [];
    for (const { target, draft } of db
      .prepare("SELECT target, draft FROM hidden_details WHERE environment = ?")
      .iterate(environment)) {
      if (typeof target === "string") (draft ? drafts : threads).push(target);
    }
    return { threads, drafts };
  });
}

export function setThreadDetailsVisibility(
  open: () => NodeSqlite.DatabaseSync,
  environment: string,
  target: string,
  draft: boolean,
  visible: boolean,
) {
  withLayout(open, (db) => {
    db.prepare(
      visible
        ? "DELETE FROM hidden_details WHERE environment = ? AND draft = ? AND target = ?"
        : "INSERT OR IGNORE INTO hidden_details(environment, draft, target) VALUES (?, ?, ?)",
    ).run(environment, Number(draft), target);
  });
}
