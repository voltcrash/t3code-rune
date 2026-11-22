import * as NodeFS from "node:fs";
import * as NodePath from "node:path";

const archive = process.env.T3_SMOKE_UPDATE_ASSET;
const manifest = process.env.T3_SMOKE_UPDATE_MANIFEST;
const repository = process.env.T3_SMOKE_UPDATE_REPOSITORY;
const tag = "v" + process.env.T3_SMOKE_UPDATE_VERSION;
const releases = `https://github.com/${repository}/releases`;

const fetchFromNetwork = globalThis.fetch;
if (NodePath.basename(process.argv[1] ?? "") === "rune-projects.mjs")
  globalThis.fetch = async (input, options) => {
    const url = String(input);
    if (url === `${releases}/download/update-feed/nightly.json`) {
      return Response.json({ tag_name: tag });
    }
    if (url === `${releases}/download/${tag}/${NodePath.basename(archive)}`) {
      return new Response(await NodeFS.openAsBlob(archive));
    }
    for (const base of [`${releases}/latest/download`, `${releases}/download/${tag}`]) {
      if (url === `${base}/${NodePath.basename(manifest)}`)
        return new Response(await NodeFS.openAsBlob(manifest));
    }
    if (
      url.startsWith(releases + "/") ||
      url.startsWith(`https://api.github.com/repos/${repository}/releases`)
    )
      return new Response(null, { status: 404 });
    return fetchFromNetwork(input, options);
  };
