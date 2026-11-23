import { layerRemoteHttpClient } from "@t3tools/client-runtime/rpc";
import { DEFAULT_SPEECH_TRANSCRIPTION_OPTIONS } from "@t3tools/contracts";
import { expect, it, vi } from "vite-plus/test";
import { workspaceDictation } from "./rune-dictation.mjs";

it("sends a take only to the environment and origin granted when recording started", async () => {
  let preference;
  const started = Promise.withResolvers();
  const pcm = Promise.withResolvers();
  const posted = [];
  const report = vi.fn();
  const workspace = (environmentId, httpBaseUrl) =>
    workspaceDictation({
      owner: {
        environmentId,
        httpBaseUrl,
        token: "fixture",
        label: "Voice",
        canOperate: true,
        voiceTranscription: true,
        http: layerRemoteHttpClient(async (input, init) => {
          if (init?.method === "POST") {
            posted.push(String(input));
            return Response.json({ text: "spoken" });
          }
          return Response.json({
            supported: true,
            state: "ready",
            modelId: "model",
            model: "Fixture",
            size: 1,
            supportsStreaming: false,
            supportsTranslation: false,
            acceleration: "cpu",
            modelUnloadTimeout: "never",
            gpuDevices: [],
          });
        }),
      },
      signal: new AbortController().signal,
      local: false,
      report,
      read: () => preference,
      save: (_, value) => {
        preference = value;
      },
      settings: () => DEFAULT_SPEECH_TRANSCRIPTION_OPTIONS,
      capture: async (_, signal, ready) => {
        signal.throwIfAborted();
        ready(() => {});
        started.resolve();
        return pcm.promise;
      },
    });
  const origin = "https://one.example.test";
  const first = workspace("one", origin);
  const moved = workspace("one", "https://two.example.test");
  const other = workspace("two", origin);
  const draft = { text: "before", selection: { start: 6, end: 6 } };
  expect(first.choice().enabled).toBe(false);
  first.select(true);
  expect(moved.choice().enabled).toBe(false);
  expect(other.choice().enabled).toBe(false);
  const pending = first.start(1, draft);
  await started.promise;
  const replacement = workspace("one", origin);
  replacement.select(false);
  replacement.select(true);
  pcm.resolve(Buffer.from([0, 0, 0, 0]));
  await pending;
  expect(posted).toEqual([]);
  await replacement.start(2, draft);
  expect(posted).toEqual([origin + "/api/voice/transcribe"]);
  expect(report.mock.lastCall[0]).toMatchObject({
    operation: 2,
    phase: "transcript",
    edit: { start: 6, end: 6, text: " spoken" },
  });
  replacement.insert(2, { text: "🧪 " + draft.text, selection: { start: 0, end: 0 } });
  expect(report.mock.lastCall[0].edit).toEqual({ start: 11, end: 11, text: " spoken" });
  replacement.finish(2, true);
});
