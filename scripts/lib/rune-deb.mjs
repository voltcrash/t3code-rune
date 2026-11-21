import * as NodeChildProcess from "node:child_process";
import * as NodeFSP from "node:fs/promises";
import * as NodePath from "node:path";

const maintainer = "T3 Tools <hello@t3.codes>";
const description = "Desktop GUI for coding agents";
const commonControl = `Section: devel
Priority: optional
Maintainer: ${maintainer}
Homepage: https://t3.codes`;

// dpkg-shlibdeps reads ELF symbol versions, including those required by bundled Node addons.
async function runtimeDependencies(root, tmp, name) {
  const files = NodeChildProcess.execFileSync(
    "find",
    [
      ".",
      "-type",
      "f",
      "!",
      "-ipath",
      "*musl*",
      "(",
      "-perm",
      "/111",
      "-o",
      "-name",
      "*.node",
      "-o",
      "-name",
      "*.so*",
      ")",
      "-print0",
    ],
    { cwd: root, encoding: "utf8" },
  )
    .split("\0")
    .filter(Boolean);
  const executables = [];
  const magic = Buffer.alloc(4);
  for (const path of files) {
    const file = NodePath.join(root, path);
    await using handle = await NodeFSP.open(file);
    const { bytesRead } = await handle.read(magic, 0, 4, 0);
    if (bytesRead === 4 && magic.readUInt32BE() === 0x7f454c46) executables.push("-e" + file);
  }
  const sourceDir = NodePath.join(tmp, "debian");
  await NodeFSP.mkdir(sourceDir, { recursive: true });
  await NodeFSP.writeFile(
    NodePath.join(sourceDir, "control"),
    `Source: ${name}
${commonControl}

Package: ${name}
Architecture: any
Description: ${description}
`,
  );
  const line = NodeChildProcess.execFileSync(
    "dpkg-shlibdeps",
    ["--warnings=0", "-O", "-S" + root, "-x" + name, ...executables],
    { cwd: tmp, encoding: "utf8" },
  ).trim();
  const deps = /^shlibs:Depends=(.+)$/m.exec(line)?.[1];
  if (!deps) throw new Error("dpkg-shlibdeps returned no Rune runtime dependencies.");
  // Winit and wgpu open display libraries at runtime. Keep any stronger ELF-derived minimums.
  const linked = new Set(deps.split(", ").map((entry) => entry.split(" ")[0]));
  const display = [
    "libvulkan1",
    "libx11-6",
    "libx11-xcb1",
    "libxi6",
    "libxcursor1",
    "libxrandr2",
    "libxkbcommon-x11-0",
    "libwayland-client0",
    "libwayland-cursor0",
    "xdg-utils",
  ];
  return [deps, ...display.filter((name) => !linked.has(name))].join(", ");
}

export async function buildRuneDeb({
  root,
  tmp,
  artifact,
  architecture,
  version,
  product,
  desktopId,
  linuxUpdate,
  desktop,
  license,
}) {
  const host = NodeChildProcess.execFileSync("dpkg", ["--print-architecture"], {
    encoding: "utf8",
  }).trim();
  if (host !== architecture)
    throw new Error(`Build the ${architecture} package on a matching Debian or Ubuntu host.`);
  const name = linuxUpdate ? "t3code" : "t3code-rune";
  const metaId = linuxUpdate ? "com.t3tools.t3code" : desktopId;
  const meta = NodePath.join(root, "usr/share/metainfo");
  const docs = NodePath.join(root, "usr/share/doc", name);
  const control = NodePath.join(root, "DEBIAN");
  await NodeFSP.mkdir(meta, { recursive: true });
  await NodeFSP.mkdir(docs, { recursive: true });
  await NodeFSP.mkdir(control, { recursive: true });
  const xml = await NodeFSP.readFile(
    NodePath.join(desktop, "resources/linux/com.t3tools.t3code.metainfo.xml"),
    "utf8",
  );
  await NodeFSP.writeFile(
    NodePath.join(meta, metaId + ".metainfo.xml"),
    xml
      .replace(/<id>[^<]+<\/id>/, `<id>${metaId}</id>`)
      .replace(/<name>[^<]+<\/name>/, `<name>${product}</name>`)
      .replace(/(<launchable type="desktop-id">)[^<]+/, `$1${desktopId}.desktop`)
      .replace(/(<icon type="stock">)[^<]+/, `$1${desktopId}`)
      .replace(/<binary>[^<]+<\/binary>/, `<binary>${linuxUpdate ? "t3code" : "t3-code"}</binary>`),
  );
  await NodeFSP.copyFile(license, NodePath.join(docs, "copyright"));
  const depends = await runtimeDependencies(root, tmp, name);
  const installedSize = NodeChildProcess.execFileSync(
    "du",
    ["--apparent-size", "--block-size=1024", "--summarize", "."],
    { cwd: root, encoding: "utf8" },
  ).split(/\s/)[0];
  if (linuxUpdate) {
    const binary = `'/opt/${product}/t3code'`;
    await NodeFSP.writeFile(
      NodePath.join(control, "postinst"),
      `#!/bin/sh\nset -e\nif [ "$1" = configure ]; then update-alternatives --install /usr/bin/t3code t3code ${binary} 100; fi\n`,
      { mode: 0o755 },
    );
    await NodeFSP.writeFile(
      NodePath.join(control, "postrm"),
      `#!/bin/sh\nset -e\ncase "$1" in remove|disappear|upgrade) if [ ! -e ${binary} ]; then update-alternatives --remove t3code ${binary}; fi ;; esac\n`,
      { mode: 0o755 },
    );
  }
  const debVersion = version.replace(/^(\d+\.\d+\.\d+)-/, "$1~") + "-1";
  await NodeFSP.writeFile(
    NodePath.join(control, "control"),
    `Package: ${name}
Version: ${debVersion}
Architecture: ${architecture}
Installed-Size: ${installedSize}
${commonControl}
Depends: ${depends}
Recommends: mesa-vulkan-drivers | vulkan-icd
Description: ${description}
 Run coding agents locally and connect to your T3 Code environments.
`,
  );
  NodeChildProcess.execFileSync("chmod", ["-R", "u=rwX,go=rX", root]);
  NodeChildProcess.execFileSync(
    "dpkg-deb",
    ["--root-owner-group", "-Zxz", "-z3", "--build", root, artifact],
    { stdio: "inherit" },
  );
}
