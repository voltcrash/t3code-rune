import { buildRemoteOpenUrl, EDITORS, WS_METHODS } from "@t3tools/contracts";
import * as NodeServices from "@effect/platform-node/NodeServices";
import * as Effect from "effect/Effect";
import { msg } from "gt-i18n";
import * as NodeUtil from "node:util";
import { findRemoteEditors } from "../src/app/remoteEditors.ts";
import { m, onLocaleChange } from "../src/i18n/catalog.ts";
import { openExternalUrl } from "./rune-external.mjs";

function editorLabel(editor, platform) {
  if (editor.id !== "file-manager") return editor.label;
  if (platform === "darwin") return "Finder";
  return platform === "windows"
    ? m(msg("File Explorer", { $context: "the Windows file manager app" }))
    : m(msg("Files", { $context: "the Linux file manager app" }));
}

export function workspaceEditors({
  local,
  session,
  signal,
  preferred,
  save,
  sshHintSeen,
  markSshHintSeen,
  publish,
}) {
  let config;
  let rpc;
  let detected;
  let last;
  const options = () => (local ? (config?.availableEditors ?? []) : (detected ?? []));
  const host = () => config?.remoteOpenTargets?.[0];
  const canOpen = () =>
    Boolean(config) && (local ? session.canOperate : Boolean(host()) && Boolean(detected?.length));
  function update() {
    if (signal.aborted) return;
    const available = options();
    const editors = EDITORS.filter((editor) => available.includes(editor.id)).map((editor) => ({
      id: editor.id,
      label: editorLabel(editor, session.platform),
    }));
    const primary = editors.find((editor) => editor.id === preferred) ?? editors[0];
    const value = {
      editors,
      preferred: primary?.id,
      canOpen: canOpen(),
      canChoose: local ? session.canOperate : detected !== undefined,
      open: primary
        ? m(msg("Open in {editor}", { editor: primary.label }))
        : m(msg("Open in editor")),
      choose: m(msg("Choose editor")),
      hint:
        !local && host() && !sshHintSeen
          ? m(
              msg("Opens over SSH. Needs your key on {environment}", {
                environment: session.label,
              }),
            )
          : undefined,
      unavailable:
        !local && !host()
          ? m(msg("No SSH route to {environment}", { environment: session.label }))
          : editors.length
            ? undefined
            : m(msg("No installed editors found")),
    };
    if (!NodeUtil.isDeepStrictEqual(last, value)) {
      last = value;
      publish(value);
    }
  }
  update();
  signal.addEventListener("abort", onLocaleChange(update), { once: true });
  if (!local) {
    void Effect.runPromise(findRemoteEditors().pipe(Effect.provide(NodeServices.layer)), { signal })
      .catch(() => [])
      .then((ids) => {
        detected = ids;
        update();
      });
  }
  return {
    catalog: () => last,
    configure(next, connected) {
      rpc = connected;
      if (last && config === next) return;
      const changed =
        config?.availableEditors !== next?.availableEditors ||
        config?.remoteOpenTargets !== next?.remoteOpenTargets ||
        Boolean(config) !== Boolean(next);
      config = next;
      if (changed || !last) update();
    },
    choose(id) {
      if (signal.aborted || !canOpen() || !options().includes(id)) return false;
      save(id);
      preferred = id;
      update();
      return true;
    },
    async open(id, cwd) {
      if (signal.aborted || !cwd || !canOpen() || !options().includes(id)) return false;
      if (local) {
        if (!rpc) return false;
        await Effect.runPromise(
          rpc[WS_METHODS.shellOpenInEditor]({ editor: id, cwd, position: {} }).pipe(
            Effect.timeout("20 seconds"),
          ),
          { signal },
        );
        return true;
      }
      const url = buildRemoteOpenUrl({ editor: id, ...host(), absolutePath: cwd });
      if (!url || !(await openExternalUrl(url))) return false;
      if (!signal.aborted && !sshHintSeen) {
        sshHintSeen = true;
        try {
          markSshHintSeen();
        } catch {}
        update();
      }
      return true;
    },
  };
}
