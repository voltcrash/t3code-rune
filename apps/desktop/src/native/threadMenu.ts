import { msg } from "gt-i18n";
import { m } from "../i18n/catalog.ts";

export function threadMenuLabels() {
  return {
    pin: m(msg("Pin thread")),
    unpin: m(msg("Unpin thread")),
    settle: m(msg("Settle thread")),
    unsettle: m(msg("Un-settle thread")),
    rename: m(msg("Rename thread")),
    mark_unread: m(msg("Mark unread")),
    copy: m(msg("Copy", { $context: "submenu of things to copy" })),
    copy_path: m(msg("Path", { $context: "copy the file path" })),
    copy_branch: m(msg("Branch", { $context: "copy the git branch name" })),
    copy_thread_id: m(msg("Thread ID")),
    delete: m(msg("Delete", { $context: "verb, delete the thread" })),
    delete_title: m(msg('Delete thread "{title}"?', { title: "{title}" })),
    delete_description: m(msg("This permanently clears conversation history for this thread.")),
    cancel: m(msg("Cancel")),
    confirm: m(msg("Confirm")),
  };
}
