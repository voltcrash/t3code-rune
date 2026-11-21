import * as NodeFSP from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import { expect, it } from "vitest";
import { publishRuneArtifacts } from "./rune-artifacts.mjs";

it.each([true, false])(
  "restores the release when the second publication fails (existing AppImage: %s)",
  async (existing) => {
    await using output = await NodeFSP.mkdtempDisposable(
      NodePath.join(NodeOS.tmpdir(), "rune-release-"),
    );
    const staged = NodePath.join(output.path, "stage");
    await NodeFSP.mkdir(staged);
    const image = NodePath.join(output.path, "app.AppImage");
    if (existing) await NodeFSP.writeFile(image, "previous");
    await NodeFSP.writeFile(NodePath.join(staged, "app.AppImage"), "replacement");
    const deb = NodePath.join(output.path, "app.deb");
    await NodeFSP.writeFile(deb, "previous deb");
    await expect(
      publishRuneArtifacts(staged, output.path, ["app.AppImage", "app.deb"]),
    ).rejects.toThrow();
    if (existing) expect(await NodeFSP.readFile(image, "utf8")).toBe("previous");
    else await expect(NodeFSP.access(image)).rejects.toMatchObject({ code: "ENOENT" });
    expect(await NodeFSP.readFile(deb, "utf8")).toBe("previous deb");
  },
);
