import {
  desktopUpdateChannelOf,
  releaseAssetUrl,
  updateFeedFileName,
} from "@t3tools/shared/cliRelease";
import { fromYaml } from "@t3tools/shared/schemaYaml";
import { compareSemverVersions } from "@t3tools/shared/semver";
import * as Schema from "effect/Schema";
import type { UpdatePolicy } from "../updates/DesktopUpdatePolicy.ts";

const ReleaseSource = Schema.Struct({
  repository: Schema.NullOr(Schema.String.check(Schema.isPattern(/^[\w.-]+\/[\w.-]+$/))),
  arch: Schema.Literals(["arm64", "x64", "universal"]),
  format: Schema.Literals(["zip", "exe", "deb", "AppImage"]),
});
export const decodeReleaseSource = Schema.decodeUnknownSync(Schema.fromJsonString(ReleaseSource));
const decodeManifest = Schema.decodeUnknownSync(
  fromYaml(
    Schema.Struct({
      version: Schema.String,
      files: Schema.Array(
        Schema.Struct({ url: Schema.String, size: Schema.Int, sha512: Schema.String }),
      ),
    }),
  ),
);
const nightlyTag = /^v\d+\.\d+\.\d+-nightly\.\d{8}\.\d+$/;
const decodePointer = Schema.decodeUnknownSync(
  Schema.fromJsonString(Schema.Struct({ tag_name: Schema.String })),
);
const decodeReleases = Schema.decodeUnknownSync(
  Schema.fromJsonString(
    Schema.Array(
      Schema.Struct({
        tag_name: Schema.String,
        draft: Schema.Boolean,
      }),
    ),
  ),
);

async function newestNightly(
  repository: string,
  current: string,
  read: (url: string) => Promise<string | undefined>,
) {
  let tag: string | undefined;
  try {
    const pointer = decodePointer(
      await read(`https://github.com/${repository}/releases/download/update-feed/nightly.json`),
    ).tag_name;
    if (nightlyTag.test(pointer)) tag = pointer;
  } catch {
    // Older repositories can publish tagged nightlies without the rolling pointer.
  }
  if (tag && compareSemverVersions(tag.slice(1), current) > 0) return tag;
  try {
    for (let page = 1; page <= 10; page++) {
      const releases = decodeReleases(
        await read(`https://api.github.com/repos/${repository}/releases?per_page=100&page=${page}`),
      );
      const latest = releases.find((r) => !r.draft && nightlyTag.test(r.tag_name))?.tag_name;
      if (latest)
        return !tag || compareSemverVersions(latest.slice(1), tag.slice(1)) > 0 ? latest : tag;
      if (releases.length < 100) break;
    }
  } catch {
    if (!tag) throw new Error("Could not find the latest nightly release.");
  }
  if (!tag) throw new Error("Could not find a nightly release.");
  return tag;
}

export async function checkRuneRelease(
  current: string,
  source: typeof ReleaseSource.Type | undefined,
  policy: UpdatePolicy,
  read: (url: string) => Promise<string | undefined>,
) {
  const channel = desktopUpdateChannelOf(current);
  if (policy.mode === "off" || (channel === "nightly" && !policy.allowNightly)) {
    return {
      status: "disabled",
      message: policy.message ?? "Updates are disabled by your administrator.",
    } as const;
  }
  if (!source || (!source.repository && !policy.feedUrl)) {
    return { status: "disabled", message: "This build has no configured update feed." } as const;
  }
  const platform = source.format === "zip" ? "darwin" : source.format === "exe" ? "win32" : "linux";
  const filename = updateFeedFileName(channel, platform, source.arch);
  let base = policy.feedUrl;
  let tag: string | undefined;
  if (!base) {
    const repository = source.repository!;
    const releases = `https://github.com/${repository}/releases`;
    if (channel === "nightly") tag = await newestNightly(repository, current, read);
    base = tag
      ? releaseAssetUrl(releases, "download", tag)
      : releaseAssetUrl(releases, "latest", "download");
  }
  let body = await read(releaseAssetUrl(base, filename));
  if (body === undefined) {
    const standalone =
      platform === "win32"
        ? `${channel}-win-${source.arch}.yml`
        : platform === "darwin" && source.arch === "x64"
          ? `${channel}-mac-x64.yml`
          : undefined;
    if (standalone) body = await read(releaseAssetUrl(base, standalone));
  }
  const manifest = decodeManifest(body);
  const version = manifest.version;
  const valid =
    channel === "nightly" ? nightlyTag.test(`v${version}`) : /^\d+\.\d+\.\d+$/.test(version);
  if (!valid || (tag && tag !== `v${version}`)) throw new Error("Unexpected release version.");
  if (compareSemverVersions(version, current) <= 0) return { status: "current" } as const;
  const arch = source.arch === "x64" && source.format === "deb" ? "amd64" : source.arch;
  const name =
    source.format === "AppImage"
      ? `T3-Code${channel === "nightly" ? "-Nightly" : ""}-${arch === "x64" ? "x86_64" : arch}.AppImage`
      : `T3-Code-${version}-${arch}.${source.format}`;
  const file = manifest.files.find((entry) => entry.url === name);
  if (!file || file.size <= 0 || !/^[A-Za-z0-9+/]{86}==$/.test(file.sha512)) {
    throw new Error("The release has no valid download for this installation.");
  }
  const downloadBase =
    policy.feedUrl ?? `https://github.com/${source.repository}/releases/download/v${version}`;
  return {
    status: "available",
    version,
    url: releaseAssetUrl(downloadBase, name),
    filename: name,
    sha512: file.sha512,
    size: file.size,
  } as const;
}
