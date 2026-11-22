import { EDITORS, REMOTE_CAPABLE_EDITOR_IDS } from "@t3tools/contracts";
import * as HostProcess from "@t3tools/shared/HostProcess";
import { resolveEditorCommand } from "@t3tools/shared/editor";
import { withPathDirectoryListings } from "@t3tools/shared/shell";
import * as Effect from "effect/Effect";
import * as Option from "effect/Option";

export const findRemoteEditors = Effect.fn("desktop.findRemoteEditors")(function* () {
  const env = yield* HostProcess.Environment;
  // Shared listings stop each editor re-stating every PATH entry x PATHEXT candidate on Windows.
  const resolved = yield* Effect.forEach(
    REMOTE_CAPABLE_EDITOR_IDS,
    (editorId) => {
      const editor = EDITORS.find((editor) => editor.id === editorId);
      return editor
        ? resolveEditorCommand(editor, env).pipe(Effect.map(Option.isSome))
        : Effect.succeed(false);
    },
    { concurrency: 8 },
  ).pipe(withPathDirectoryListings);
  return REMOTE_CAPABLE_EDITOR_IDS.filter((_, index) => resolved[index]);
});
