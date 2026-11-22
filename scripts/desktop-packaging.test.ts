// @effect-diagnostics nodeBuiltinImport:off - Compare the staged archive digest against the original bytes.
import * as NodeCrypto from "node:crypto";
import * as NodeServices from "@effect/platform-node/NodeServices";
import { assert, it } from "@effect/vitest";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import { ChildProcess, ChildProcessSpawner } from "effect/process";

import {
  parseWslRuntimeArchiveMembers,
  stageWslRuntimeArchive,
  validatePackagedWslRuntime,
  WSL_RUNTIME_ARCHIVE_HASH_NAME,
  WSL_RUNTIME_ARCHIVE_NAME,
} from "./desktop-packaging.ts";

it.layer(NodeServices.layer)("Windows desktop WSL runtime", (it) => {
  it("parses Windows bsdtar member listings with CRLF line endings", () => {
    assert.deepStrictEqual(parseWslRuntimeArchiveMembers("./t3/t3\r\nt3/client/\r\n"), [
      "t3/t3",
      "t3/client",
    ]);
  });

  it.effect.each([
    { arch: "x64", pty: "build/Release/pty.node", valid: true },
    { arch: "x64", pty: "prebuilds/linux-x64/pty.node", valid: true },
    { arch: "arm64", pty: "prebuilds/linux-arm64/pty.node", valid: true },
    { arch: "arm64", pty: "prebuilds/linux-x64/pty.node", valid: false },
    { arch: "x64", pty: "prebuilds/linux-arm64/pty.node", valid: false },
    { arch: "x64", pty: "package.json", valid: false },
  ] as const)("validates $pty for $arch", ({ arch, pty, valid }) =>
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const path = yield* Path.Path;
      const spawner = yield* ChildProcessSpawner.ChildProcessSpawner;
      const root = yield* fs.makeTempDirectoryScoped();
      const input = path.join(root, "input");
      const version = "1.2.3";
      const stem = `t3-${version}-linux-${arch}`;
      for (const member of ["t3", "client/index.html", `node_modules/node-pty/${pty}`]) {
        const file = path.join(input, stem, member);
        yield* fs.makeDirectory(path.dirname(file), { recursive: true });
        yield* fs.writeFileString(file, member);
      }
      const sourceArchivePath = path.join(root, "source.tar.gz");
      const tar = yield* spawner.spawn(
        ChildProcess.make("tar", ["-czf", sourceArchivePath, "-C", input, stem]),
      );
      assert.equal(Number(yield* tar.exitCode), 0);
      const archivePath = path.join(root, "resources", WSL_RUNTIME_ARCHIVE_NAME);
      const hashPath = path.join(root, "resources", WSL_RUNTIME_ARCHIVE_HASH_NAME);
      yield* stageWslRuntimeArchive({ sourceArchivePath, archivePath, hashPath });
      const validate = (appVersion: string) =>
        validatePackagedWslRuntime({ packagedAppDir: root, targetArch: arch, appVersion });

      if (!valid) {
        const error = yield* Effect.flip(validate(version));
        assert.include(
          error.missingFiles ?? [],
          `${stem}/node_modules/node-pty/build/Release/pty.node`,
        );
        return;
      }
      yield* validate(version);
      const bytes = yield* fs.readFile(sourceArchivePath);
      assert.deepStrictEqual(yield* fs.readFile(archivePath), bytes);
      assert.equal(
        yield* fs.readFileString(hashPath),
        NodeCrypto.createHash("sha256").update(bytes).digest("hex") + "\n",
      );
      assert.include(
        String((yield* Effect.flip(validate("2.0.0"))).cause),
        "single top-level directory",
      );
      yield* fs.writeFileString(hashPath, "0".repeat(64));
      assert.include(String((yield* Effect.flip(validate(version))).cause), "SHA-256 mismatch");
    }),
  );
});
