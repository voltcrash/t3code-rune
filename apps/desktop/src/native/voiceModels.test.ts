import { layerRemoteHttpClient } from "@t3tools/client-runtime/rpc";
import { EnvironmentId } from "@t3tools/contracts";
import { expect, it, vi } from "vite-plus/test";

import { watchVoiceModels } from "./voiceModels.ts";

const status = {
  supported: true,
  state: "ready",
  modelId: "local",
  model: "Local",
  size: 100,
  supportsStreaming: false,
  supportsTranslation: false,
  acceleration: "cpu",
  modelUnloadTimeout: "never",
  gpuDevices: [],
};

const model = {
  id: "local",
  name: "Local",
  state: "installed",
  description: "Local",
  size: 100,
  languages: ["en"],
  accuracy: 1,
  speed: 1,
  recommended: false,
  supportsStreaming: false,
  supportsLanguageDetection: false,
  supportsTranslation: false,
  active: false,
};

it("keeps a late catalog out of a reopened page and restricts changes to manageable listed models", async () => {
  const pending = Promise.withResolvers<void>();
  const first = Promise.withResolvers<Response>();
  const mutations: string[] = [];
  let requested = false;
  let active = false;
  const session = {
    environmentId: EnvironmentId.make("voice-test"),
    label: "Voice",
    httpBaseUrl: "https://example.test",
    token: "fixture",
    canWriteSettings: false,
    http: layerRemoteHttpClient(async (input, init) => {
      const path = new URL(String(input)).pathname;
      if (path.endsWith("/status")) {
        return active ? new Response(null, { status: 403 }) : Response.json(status);
      }
      if (!requested) {
        requested = true;
        pending.resolve();
        return first.promise;
      }
      if (init?.method === "POST") {
        mutations.push(path);
        active = true;
      }
      return Response.json({ models: [{ ...model, active }] });
    }),
  };
  const owner = new AbortController();
  const publish = vi.fn<Parameters<typeof watchVoiceModels>[2]>();
  const page = watchVoiceModels(session, owner.signal, publish);
  const opening = page.show(true);
  await pending.promise;
  page.show(false);
  await page.show(true);
  first.resolve(Response.json({ models: [] }));
  await opening;
  expect(publish).toHaveBeenCalledTimes(1);
  expect(publish.mock.lastCall?.[0]).toMatchObject({ models: [{ id: "local" }], manage: false });
  await page.change("select", "local");
  session.canWriteSettings = true;
  await page.change("select", "missing");
  expect(mutations).toEqual([]);
  await page.change("select", "local");
  expect(publish.mock.lastCall?.[0]).toMatchObject({
    models: [{ active: true }],
    message: "Could not load transcription models",
    busy: false,
  });
  owner.abort();
  await page.change("remove", "local");
  expect(mutations).toEqual(["/api/voice/models/select"]);
});
