import * as NodeFSP from "node:fs/promises";
import * as NodePath from "node:path";

export async function publishRuneArtifacts(staged, output, names, { requireFresh = false } = {}) {
  const previous = new Set();
  if (requireFresh || names.length > 1) {
    for (const name of names) {
      try {
        const destination = NodePath.join(output, name);
        if (requireFresh) {
          await NodeFSP.lstat(destination);
          throw new Error(
            `Update asset already exists: ${destination}. Choose a fresh output directory.`,
          );
        }
        await NodeFSP.cp(destination, NodePath.join(staged, name + ".previous"), {
          mode: NodeFSP.constants.COPYFILE_FICLONE,
          verbatimSymlinks: true,
        });
        previous.add(name);
      } catch (error) {
        if (error.code !== "ENOENT") throw error;
      }
    }
  }

  const published = [];
  try {
    for (const name of names) {
      await NodeFSP.rename(NodePath.join(staged, name), NodePath.join(output, name));
      published.push(name);
    }
  } catch (error) {
    for (const name of published) {
      const destination = NodePath.join(output, name);
      if (previous.has(name))
        await NodeFSP.rename(NodePath.join(staged, name + ".previous"), destination);
      else await NodeFSP.unlink(destination);
    }
    throw error;
  }
}

export async function publishUnpackedRuneApp(staged, output, name) {
  const app = NodePath.join(staged, name);
  const destination = NodePath.join(output, name);
  const backup = NodePath.join(staged, ".previous");
  let replaced = false;
  try {
    await NodeFSP.rename(destination, backup);
    replaced = true;
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  try {
    await NodeFSP.rename(app, destination);
  } catch (error) {
    if (replaced) await NodeFSP.rename(backup, destination);
    throw error;
  }
}
