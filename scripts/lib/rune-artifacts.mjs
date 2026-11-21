import * as NodeFSP from "node:fs/promises";
import * as NodePath from "node:path";

export async function publishRuneArtifacts(staged, output, names) {
  const previous = new Set();
  if (names.length > 1) {
    for (const name of names) {
      try {
        await NodeFSP.cp(NodePath.join(output, name), NodePath.join(staged, name + ".previous"), {
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
