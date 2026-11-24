import { msg } from "gt-i18n";
import { m } from "../i18n/catalog.ts";

export function threadMenuLabels() {
  return {
    pin: m(msg("Pin thread")),
    unpin: m(msg("Unpin thread")),
    settle: m(msg("Settle thread")),
    unsettle: m(msg("Un-settle thread")),
    snooze: m(msg("Snooze", { $context: "verb, snooze the thread" })),
    wake: m(msg("Wake thread")),
    rename: m(msg("Rename thread")),
    mark_unread: m(msg("Mark unread")),
    copy: m(msg("Copy", { $context: "submenu of things to copy" })),
    copy_path: m(msg("Path", { $context: "copy the file path" })),
    copy_branch: m(msg("Branch", { $context: "copy the git branch name" })),
    copy_thread_id: m(msg("Thread ID")),
    unpin_title: m(msg('Unpin thread "{title}"?', { title: "{title}" })),
    unpin_description: m(msg("This will move the thread out of your pinned section.")),
    archive: m(msg("Archive thread")),
    archive_title: m(msg('Archive thread "{title}"?', { title: "{title}" })),
    delete: m(msg("Delete", { $context: "verb, delete the thread" })),
    delete_title: m(msg('Delete thread "{title}"?', { title: "{title}" })),
    delete_description: m(msg("This permanently clears conversation history for this thread.")),
    cancel: m(msg("Cancel")),
    confirm: m(msg("Confirm")),
  };
}

export function archivedPageLabels() {
  return {
    title: m(msg("Archive")),
    empty: m(msg("No archived threads")),
    empty_description: m(msg("Archived threads will appear here.")),
    loading: m(msg("Loading archived threads")),
    loading_description: m(msg("Checking connected environments.")),
    failed: m(msg("Could not load archived threads")),
    unarchive: m(msg("Unarchive")),
    unarchive_thread: m(msg("Unarchive {title}", { title: "{title}" })),
    delete: m(msg("Delete", { $context: "verb, deletes a thread" })),
    delete_thread: m(msg("Delete {title}", { title: "{title}" })),
    dates: m(
      msg("Archived {archived} · Created {created}", {
        archived: "{archived}",
        created: "{created}",
      }),
    ),
    ago: m(msg("{age} ago", { age: "{age}" })),
    just_now: m(msg("just now")),
  };
}

export function generalPageLabels() {
  return {
    title: m(msg("General")),
    confirmations: m(msg("Confirmations")),
    unpin: m(msg("Unpin confirmation")),
    unpin_description: m(msg("Ask before unpinning a thread from the pinned section.")),
    unpin_reset: m(msg("Reset unpin confirmation to default")),
    archive: m(msg("Archive confirmation")),
    archive_description: m(
      msg("Require a second click on the inline archive action before a thread is archived."),
    ),
    archive_reset: m(msg("Reset archive confirmation to default")),
    delete: m(msg("Delete confirmation")),
    delete_description: m(msg("Ask before deleting a thread and its chat history.")),
    delete_reset: m(msg("Reset delete confirmation to default")),
  };
}
