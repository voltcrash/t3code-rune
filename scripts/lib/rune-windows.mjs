import * as NodeFSP from "node:fs/promises";
import { Data, NtExecutable, NtExecutableResource, Resource } from "resedit";

export async function windowsNodeArchitecture(path, version) {
  const exe = NtExecutable.from(await NodeFSP.readFile(path), { ignoreCert: true });
  const arch = { [0x8664]: "x64", [0xaa64]: "arm64" }[exe.newHeader.fileHeader.machine];
  const info = Resource.VersionInfo.fromEntries(NtExecutableResource.from(exe).entries)[0];
  const [major, minor, patch] = version.split(".").map(Number);
  if (
    !arch ||
    !info ||
    info.fixedInfo.fileVersionMS !== major * 65536 + minor ||
    info.fixedInfo.fileVersionLS !== patch * 65536
  ) {
    throw new Error(`Use a Windows x64 or ARM64 Node ${version} distribution to package Rune.`);
  }
  return arch;
}

export async function stampWindowsExecutable(path, iconPath, version, product) {
  const exe = NtExecutable.from(await NodeFSP.readFile(path));
  const resources = NtExecutableResource.from(exe);
  const icon = Data.IconFile.from(await NodeFSP.readFile(iconPath));
  Resource.IconGroupEntry.replaceIconsForResource(
    resources.entries,
    1,
    1033,
    icon.icons.map((entry) => entry.data),
  );
  const info = Resource.VersionInfo.createEmpty();
  info.lang = 1033;
  const [numericVersion] = version.split(/[-+]/, 1);
  info.setFileVersion(numericVersion, 1033);
  info.setProductVersion(numericVersion, 1033);
  info.setStringValues(
    { lang: 1033, codepage: 1200 },
    {
      FileDescription: product,
      ProductName: product,
      CompanyName: "T3 Tools",
      ProductVersion: version,
      OriginalFilename: product + ".exe",
    },
  );
  info.outputToResourceEntries(resources.entries);
  resources.outputResource(exe);
  await NodeFSP.writeFile(path, Buffer.from(exe.generate()));
}
