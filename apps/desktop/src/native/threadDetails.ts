import type * as NodeSqlite from "node:sqlite";
import { msg } from "gt-i18n";
import { m } from "../i18n/catalog.ts";

export function threadDetailsLabels() {
  return {
    panel: m(msg("Thread details")),
    scripts: m(msg("Choose project action")),
    importFrom: m(msg("Import from t3.json")),
    import: m(msg("Import", { $context: "verb, import an action from t3.json" })),
    form: {
      add: m(msg("Add project script")),
      addTitle: m(msg("Add Action")),
      editTitle: m(msg("Edit Action")),
      description: m(
        msg("Actions are project-scoped commands you can run from the top bar or keybindings."),
      ),
      name: m(msg("Name")),
      command: m(msg("Command")),
      preview: m(msg("Preview URL (optional)")),
      icon: m(msg("Choose icon")),
      setup: m(msg("Run automatically on worktree creation")),
      wait: m(msg("Wait for it to finish before the agent starts")),
      settle: m(msg("Run in the thread's worktree when the thread settles")),
      stop: m(msg("Stop this action when the thread settles")),
      auto: m(msg("Open preview automatically when this action runs")),
      cancel: m(msg("Cancel")),
      save: m(msg("Save action")),
      changes: m(msg("Save changes")),
      saving: m(msg("Saving…")),
      delete: m(msg("Delete", { $context: "verb, delete the action" })),
      remove: m(msg("Delete action")),
      confirm: m(msg("This action cannot be undone.")),
    },
    actions: m(msg("Actions", { $context: "project actions menu" })),
    initializeGit: m(msg("Initialize Git")),
    initializing: m(msg("Initializing...")),
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
      "CREATE TABLE IF NOT EXISTS hidden_details (environment TEXT, draft INTEGER, target TEXT, PRIMARY KEY(environment, draft, target));" +
        "CREATE TABLE IF NOT EXISTS panel_widths (environment TEXT, thread TEXT, width INTEGER NOT NULL, PRIMARY KEY(environment, thread))",
    );
    return run(db);
  } finally {
    db.close();
  }
}

export function readThreadLayout(open: () => NodeSqlite.DatabaseSync, environment: string) {
  return withLayout(open, (db) => {
    const threads: string[] = [];
    const drafts: string[] = [];
    for (const { target, draft } of db
      .prepare("SELECT target, draft FROM hidden_details WHERE environment = ?")
      .iterate(environment)) {
      if (typeof target === "string") (draft ? drafts : threads).push(target);
    }
    const widths = new Map<string, number>();
    for (const { thread, width } of db
      .prepare("SELECT thread, width FROM panel_widths WHERE environment = ?")
      .iterate(environment)) {
      if (
        typeof thread === "string" &&
        typeof width === "number" &&
        Number.isInteger(width) &&
        width >= 1 &&
        width <= 0xffffffff
      ) {
        widths.set(thread, width);
      }
    }
    return { hidden: { threads, drafts }, widths: Object.fromEntries(widths) };
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

export function setThreadPanelWidth(
  open: () => NodeSqlite.DatabaseSync,
  environment: string,
  thread: string,
  width: number,
) {
  withLayout(open, (db) => {
    db.prepare(
      "INSERT INTO panel_widths(environment, thread, width) VALUES (?, ?, ?) ON CONFLICT(environment, thread) DO UPDATE SET width = excluded.width",
    ).run(environment, thread, width);
  });
}
