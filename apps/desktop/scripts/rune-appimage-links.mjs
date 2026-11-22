import * as NodeChildProcess from "node:child_process";
import * as NodeFSP from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import * as NodeUtil from "node:util";
import * as NodeServices from "@effect/platform-node/NodeServices";
import { writeFileStringAtomically } from "@t3tools/shared/atomicWrite";
import * as Effect from "effect/Effect";
import { renderUrlHandlerDesktopEntry } from "../src/app/linuxDesktopEntry.ts";
import { installedLinuxPackage } from "./rune-linux-update.mjs";

const execFile = NodeUtil.promisify(NodeChildProcess.execFile);

export async function registerAppImageLinks(resources, env) {
  if (!NodePath.isAbsolute(env.APPIMAGE ?? "") || !NodePath.isAbsolute(env.APPDIR ?? "")) return;
  const image = await installedLinuxPackage(resources, env);
  if (image?.format !== "AppImage") return;
  const data = NodePath.isAbsolute(env.XDG_DATA_HOME ?? "")
    ? env.XDG_DATA_HOME
    : NodePath.join(NodeOS.homedir(), ".local/share");
  const dir = NodePath.join(data, "applications");
  const filePath = NodePath.join(dir, "com.t3tools.T3Code.desktop");
  const schemes = ["t3code", "t3code-dev"];
  const contents = renderUrlHandlerDesktopEntry({
    displayName: NodePath.basename(NodePath.dirname(resources)),
    execTarget: image.target,
    schemes,
  });
  const existing = await NodeFSP.readFile(filePath, "utf8").catch((error) => {
    if (error.code !== "ENOENT") throw error;
  });
  if (contents !== existing) {
    await Effect.runPromise(
      writeFileStringAtomically({ filePath, contents }).pipe(Effect.provide(NodeServices.layer)),
    );
  } else {
    const cache = await NodeFSP.stat(NodePath.join(dir, "mimeinfo.cache"), {
      throwIfNoEntry: false,
    });
    if (cache && cache.mtimeMs >= (await NodeFSP.stat(filePath)).mtimeMs) return;
  }
  try {
    await execFile("update-desktop-database", [dir], { timeout: 5000 });
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
    for (const scheme of schemes) {
      const mime = "x-scheme-handler/" + scheme;
      const { stdout } = await execFile("xdg-mime", ["query", "default", mime], { timeout: 5000 });
      if (!stdout.trim()) {
        await execFile("xdg-mime", ["default", NodePath.basename(filePath), mime], {
          timeout: 5000,
        });
      }
    }
  }
}
