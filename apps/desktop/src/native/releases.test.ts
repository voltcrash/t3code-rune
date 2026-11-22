import { expect, it, vi } from "vite-plus/test";
import { checkRuneRelease } from "./releases.ts";
import { unmanagedUpdatePolicy } from "../updates/DesktopUpdatePolicy.ts";

const sha512 = Buffer.alloc(64, 7).toString("base64");
const source = { repository: "owner/fork", arch: "arm64", format: "zip" } as const;
function manifest(version: string, filename: string) {
  return JSON.stringify({
    version,
    desktopRuntime: "rune",
    files: [{ url: filename, size: 42, sha512 }],
  });
}

it("pins a compatible installer to the offered stable tag even when the latest URL moves", async () => {
  const filename = "T3-Code-1.3.0-arm64.zip";
  const read = vi.fn(async () => manifest("1.3.0", filename));
  const result = await checkRuneRelease("1.2.0", source, unmanagedUpdatePolicy, read);
  expect(read.mock.calls).toEqual([
    ["https://github.com/owner/fork/releases/latest/download/latest-mac.yml"],
  ]);
  expect(result).toMatchObject({
    status: "available",
    filename,
    sha512,
    url: "https://github.com/owner/fork/releases/download/v1.3.0/" + filename,
  });
  await expect(checkRuneRelease("1.4.0", source, unmanagedUpdatePolicy, read)).resolves.toEqual({
    status: "current",
  });
  await expect(
    checkRuneRelease("1.2.0", { ...source, arch: "x64" }, unmanagedUpdatePolicy, read),
  ).rejects.toThrow();
});

it("honors update policy, skips unmarked packages and preserves a mirror's signed query", async () => {
  const name = "T3-Code-1.3.0-amd64.deb";
  const read = vi.fn(async () => manifest("1.3.0", name));
  const mirror = { ...unmanagedUpdatePolicy, feedUrl: "https://mirror.example/releases?key=token" };
  for (const [pkg, policy] of [
    [undefined, mirror],
    [source, { ...mirror, mode: "off" }],
  ] as const) {
    await expect(checkRuneRelease("1.2.0", pkg, policy, read)).resolves.toMatchObject({
      status: "disabled",
    });
  }
  expect(read).not.toHaveBeenCalled();
  const legacy = () =>
    Promise.resolve(JSON.stringify({ version: "1.3.0", files: [{ url: name, size: 42, sha512 }] }));
  await expect(
    checkRuneRelease("1.2.0", { repository: null, arch: "x64", format: "deb" }, mirror, legacy),
  ).resolves.toMatchObject({ status: "disabled" });
  const result = await checkRuneRelease(
    "1.2.0",
    { repository: null, arch: "x64", format: "deb" },
    mirror,
    read,
  );
  expect(read).toHaveBeenCalledWith("https://mirror.example/releases/latest-linux.yml?key=token");
  expect(result).toMatchObject({ url: "https://mirror.example/releases/" + name + "?key=token" });
});

it("recovers a newer tagged nightly when the pointer still names the installed version", async () => {
  const current = "1.3.0-nightly.20261121.3";
  const next = "1.3.0-nightly.20261121.4";
  const read = vi.fn(async (url: string) => {
    if (url.endsWith("nightly.json")) return JSON.stringify({ tag_name: "v" + current });
    if (url.includes("api.github.com"))
      return JSON.stringify([{ tag_name: "v" + next, draft: false }]);
    return manifest(next, "T3-Code-Nightly-x86_64.AppImage");
  });
  const pkg = { ...source, arch: "x64", format: "AppImage" } as const;
  await expect(
    checkRuneRelease(current, pkg, { ...unmanagedUpdatePolicy, allowNightly: false }, read),
  ).resolves.toMatchObject({ status: "disabled" });
  expect(read).not.toHaveBeenCalled();
  await expect(checkRuneRelease(current, pkg, unmanagedUpdatePolicy, read)).resolves.toMatchObject({
    version: next,
    url: `https://github.com/owner/fork/releases/download/v${next}/T3-Code-Nightly-x86_64.AppImage`,
  });
  expect(read).toHaveBeenLastCalledWith(
    `https://github.com/owner/fork/releases/download/v${next}/nightly-linux.yml`,
  );
});

it.each([
  ["zip", "x64", "latest-mac.yml", "latest-mac-x64.yml"],
  ["exe", "arm64", "latest.yml", "latest-win-arm64.yml"],
] as const)(
  "reads a standalone %s manifest only if the merged feed is missing",
  async (format, arch, merged, standalone) => {
    const body = manifest("1.3.0", `T3-Code-1.3.0-${arch}.${format}`);
    const read = vi.fn(async (url: string) => {
      if (url.endsWith(merged)) return undefined;
      return body;
    });
    const base = "https://github.com/owner/fork/releases/latest/download/";
    await expect(
      checkRuneRelease("1.2.0", { ...source, format, arch }, unmanagedUpdatePolicy, read),
    ).resolves.toMatchObject({ status: "available" });
    expect(read.mock.calls).toEqual([[base + merged], [base + standalone]]);
  },
);
