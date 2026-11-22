import { expect, it, vi } from "vite-plus/test";
import { readRegistryIcon } from "./sidebarProviders.ts";

const url = "https://cdn.agentclientprotocol.com/registry/v1/latest/test-agent.svg";

it("reads official registry artwork without following redirects or supplying credentials", async () => {
  using request = vi
    .spyOn(globalThis, "fetch")
    .mockResolvedValue(new Response("badge", { headers: { "content-type": "image/svg+xml" } }));
  const signal = new AbortController().signal;
  expect(await readRegistryIcon("https://example.org/agent.svg", signal)).toBeNull();
  expect(request).not.toHaveBeenCalled();
  expect(await readRegistryIcon(url, signal)).toBe(Buffer.from("badge").toString("base64"));
  expect(request).toHaveBeenCalledWith(url, {
    signal,
    redirect: "error",
    credentials: "omit",
    referrerPolicy: "no-referrer",
  });
});

it("limits an icon whose response omits its size", async () => {
  using _request = vi
    .spyOn(globalThis, "fetch")
    .mockResolvedValue(
      new Response(new Uint8Array(512 * 1024 + 1), { headers: { "content-type": "image/png" } }),
    );
  await expect(readRegistryIcon(url, new AbortController().signal)).rejects.toThrow("too large");
});
