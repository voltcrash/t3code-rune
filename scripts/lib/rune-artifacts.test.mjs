import * as NodeFSP from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import { expect, it } from "vitest";
import { publishRuneArtifacts, publishUnpackedRuneApp } from "./rune-artifacts.mjs";

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

it("refuses to replace any part of an update feed", async () => {
  await using output = await NodeFSP.mkdtempDisposable(
    NodePath.join(NodeOS.tmpdir(), "rune-update-"),
  );
  const staged = NodePath.join(output.path, "stage");
  await NodeFSP.mkdir(staged);
  await NodeFSP.writeFile(NodePath.join(staged, "build.zip"), "new archive");
  await NodeFSP.writeFile(NodePath.join(staged, "latest-mac.yml"), "new manifest");
  const manifest = NodePath.join(output.path, "latest-mac.yml");
  await NodeFSP.writeFile(manifest, "prior manifest");
  await expect(
    publishRuneArtifacts(staged, output.path, ["build.zip", "latest-mac.yml"], {
      requireFresh: true,
    }),
  ).rejects.toThrow(/fresh output directory/);
  expect(await NodeFSP.readFile(manifest, "utf8")).toBe("prior manifest");
  await expect(NodeFSP.access(NodePath.join(output.path, "build.zip"))).rejects.toMatchObject({
    code: "ENOENT",
  });
});

it("replaces an unpacked application and restores it if staging is incomplete", async () => {
  await using output = await NodeFSP.mkdtempDisposable(
    NodePath.join(NodeOS.tmpdir(), "rune-unpacked-"),
  );
  const staged = NodePath.join(output.path, "stage");
  const name = "mac-arm64";
  const next = NodePath.join(staged, name);
  const destination = NodePath.join(output.path, name);
  await NodeFSP.mkdir(next, { recursive: true });
  await NodeFSP.mkdir(destination);
  await NodeFSP.writeFile(NodePath.join(destination, "stale"), "old");
  await NodeFSP.writeFile(NodePath.join(next, "launcher"), "new");
  await publishUnpackedRuneApp(staged, output.path, name);
  expect(await NodeFSP.readdir(destination)).toEqual(["launcher"]);
  const incomplete = NodePath.join(output.path, "incomplete");
  await NodeFSP.mkdir(incomplete);
  await expect(publishUnpackedRuneApp(incomplete, output.path, name)).rejects.toThrow();
  expect(await NodeFSP.readFile(NodePath.join(destination, "launcher"), "utf8")).toBe("new");
});
