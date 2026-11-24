import { SAVED_VIEW_NAME_MAX_LENGTH } from "@t3tools/client-runtime/saved-views";
import { msg } from "gt-i18n";
import { m } from "../i18n/catalog.ts";

const LISTS = ["pullRequests", "issues"] as const;
type SavedViewList = (typeof LISTS)[number];

/** The web's `SavedViewsMenu` copy, for both lists. */
export function savedViewLabels() {
  const list = (kind: "pullRequest" | "issue") => ({
    menu: m(
      msg("{list, select, pullRequest {Saved pull request views} other {Saved issue views}}", {
        list: kind,
      }),
    ),
    save_title: m(
      msg("{list, select, pullRequest {Save PR view} other {Save issue view}}", { list: kind }),
    ),
    rename_title: m(
      msg("{list, select, pullRequest {Rename PR view} other {Rename issue view}}", {
        list: kind,
      }),
    ),
  });
  return {
    views: m(msg("Views", { $context: "saved filter views menu" })),
    modified: m(msg("Modified view")),
    save_current: m(msg("Save current view…")),
    update: m(msg("Update saved view")),
    rename: m(msg("Rename view…")),
    delete: m(msg("Delete view")),
    description: m(msg("Save your search, filters, and sort order on this device.")),
    name: m(msg("View name")),
    cancel: m(msg("Cancel")),
    save: m(msg("Save")),
    empty_name: m(msg("Enter a name for this view.")),
    too_long: m(msg("Use {max} characters or fewer.", { max: SAVED_VIEW_NAME_MAX_LENGTH })),
    duplicate: m(msg("A view with this name already exists.")),
    max_length: SAVED_VIEW_NAME_MAX_LENGTH,
    pull_requests: {
      ...list("pullRequest"),
      placeholder: m(msg("PRs Requesting My Review")),
    },
    issues: {
      ...list("issue"),
      placeholder: m(msg("Issues Assigned to Me")),
    },
  };
}

/** A list's views as native sends them: ids and names checked, preferences left to native. */
function validViews(views: unknown): views is ReadonlyArray<object> {
  return (
    Array.isArray(views) &&
    views.every(
      (view: unknown) =>
        typeof view === "object" &&
        view !== null &&
        "id" in view &&
        typeof view.id === "string" &&
        view.id.length > 0 &&
        "name" in view &&
        typeof view.name === "string" &&
        view.name.trim().length > 0 &&
        view.name.length <= SAVED_VIEW_NAME_MAX_LENGTH &&
        "preferences" in view &&
        typeof view.preferences === "object" &&
        view.preferences !== null,
    )
  );
}

/** Saved views per list, read from and written to one preference file on this device. */
export function savedViewStore(read: () => string | undefined, save: (value: string) => void) {
  const load = (): Record<SavedViewList, ReadonlyArray<object>> => {
    try {
      const saved: unknown = JSON.parse(read() ?? "null");
      const views = (list: SavedViewList) => {
        const value =
          typeof saved === "object" && saved !== null && list in saved
            ? (saved as Record<string, unknown>)[list]
            : undefined;
        return validViews(value) ? value : [];
      };
      return { pullRequests: views("pullRequests"), issues: views("issues") };
    } catch {
      return { pullRequests: [], issues: [] };
    }
  };
  return {
    load,
    /** Replaces one list's views; false when the request is malformed. */
    write(list: unknown, views: unknown): boolean {
      if (!LISTS.includes(list as SavedViewList) || !validViews(views)) return false;
      save(JSON.stringify({ ...load(), [list as SavedViewList]: views }));
      return true;
    },
  };
}
