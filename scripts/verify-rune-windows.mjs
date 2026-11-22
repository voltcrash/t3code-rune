import * as NodeFSP from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import * as NodeURL from "node:url";

// Run with the installed Node so emulated x64 packages load their own native modules.
const [expectedArch] = process.argv.slice(2);
const { platform, arch } = process;
if (platform !== "win32" || arch !== expectedArch) {
  throw new Error(`Expected Windows ${expectedArch}, running ${platform} ${arch}.`);
}
const resources = NodePath.dirname(process.execPath);
const entry = NodePath.join(resources, "node_modules/@ff-labs/fff-node/dist/index.js");
const { FileFinder } = await import(NodeURL.pathToFileURL(entry).href);
await using root = await NodeFSP.mkdtempDisposable(NodePath.join(NodeOS.tmpdir(), "t3-fff-"));
const result = FileFinder.create({
  basePath: root.path,
  disableWatch: true,
  disableMmapCache: true,
  disableContentIndexing: true,
});
if (!result.ok) throw new Error(result.error);
result.value.destroy();
console.log(`Loaded the packaged Windows ${expectedArch} file index.`);
