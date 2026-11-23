import * as NodeChildProcess from "node:child_process";
import * as NodeCrypto from "node:crypto";
import { SPEECH_STREAM_MAX_QUEUED_BYTES } from "@t3tools/contracts";
import {
  getEnvironmentSpeechStatus,
  resolveTranscriptCommit,
  transcribeEnvironmentPcm,
} from "@t3tools/client-runtime/voice-input";
import { effectiveSpeechLanguage } from "@t3tools/shared/speech";
import * as Effect from "effect/Effect";
import { msg } from "gt-i18n";
import { m } from "../src/i18n/catalog.ts";
import { speechConnection } from "../src/native/voiceModels.ts";

const preference = "desktop-dictation-grant";
let pendingRevocation;

async function readRecording(output, onStarted, onProgress) {
  let header = Buffer.alloc(0);
  let audio;
  let offset = 0;
  for await (let chunk of output) {
    while (chunk.length) {
      if (audio) {
        if (chunk.length > audio.length - offset) throw new Error("Unexpected microphone data");
        offset += chunk.copy(audio, offset);
        break;
      }
      const newline = chunk.indexOf(10);
      const part = newline === -1 ? chunk : chunk.subarray(0, newline);
      if (header.length + part.length > 256) throw new Error("Invalid microphone reply");
      header = Buffer.concat([header, part]);
      if (newline === -1) break;
      const reply = JSON.parse(header.toString("utf8"));
      chunk = chunk.subarray(newline + 1);
      header = Buffer.alloc(0);
      if (reply === "Started") onStarted();
      else if (Array.isArray(reply.Progress)) onProgress(...reply.Progress);
      else if (
        Number.isSafeInteger(reply.Recorded) &&
        reply.Recorded > 0 &&
        reply.Recorded <= SPEECH_STREAM_MAX_QUEUED_BYTES / 4
      ) {
        audio = Buffer.alloc(reply.Recorded * 4);
      } else throw new Error("Microphone capture did not complete");
    }
  }
  if (!audio || offset !== audio.length) throw new Error("Incomplete microphone data");
  return audio;
}

async function capturePcm(executable, signal, onStarted, onProgress) {
  const child = NodeChildProcess.spawn(executable, ["--capture-dictation"], {
    signal,
    windowsHide: true,
    stdio: ["pipe", "pipe", "ignore"],
  });
  const exited = new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", resolve);
  });
  child.stdin.on("error", () => {});
  try {
    const [pcm, code] = await Promise.all([
      readRecording(
        child.stdout,
        () => onStarted(() => child.stdin.write(Buffer.from([1]))),
        onProgress,
      ),
      exited,
    ]);
    if (code !== 0 || signal.aborted) throw new Error("Microphone capture interrupted");
    return pcm;
  } finally {
    child.kill();
  }
}

export function workspaceDictation({
  owner,
  local,
  signal,
  read,
  save,
  report,
  settings,
  capture = capturePcm,
}) {
  const origin = local ? "local" : new URL(owner.httpBaseUrl).origin;
  const available = owner.voiceTranscription && owner.canOperate;
  const connection = speechConnection(owner);
  let active;
  let completed;
  function retryRevoke() {
    return (
      pendingRevocation?.origin === origin && pendingRevocation.environment === owner.environmentId
    );
  }
  function grant() {
    if (retryRevoke()) return null;
    try {
      const value = JSON.parse(read(preference) ?? "null");
      return value?.origin === origin &&
        value.environment === owner.environmentId &&
        typeof value.id === "string"
        ? value.id
        : null;
    } catch {
      return null;
    }
  }
  function choice() {
    return {
      available,
      enabled: available && !!grant(),
      retry_revoke: retryRevoke(),
      destination: local ? owner.label : origin,
    };
  }
  function select(enabled) {
    if (signal.aborted || (enabled && (!available || retryRevoke()))) return;
    active?.controller.abort();
    completed = undefined;
    if (!enabled) pendingRevocation = { origin, environment: owner.environmentId };
    try {
      save(
        preference,
        enabled
          ? JSON.stringify({
              id: NodeCrypto.randomUUID(),
              origin,
              environment: owner.environmentId,
            })
          : null,
      );
      pendingRevocation = undefined;
      report({ type: "dictationchoice", choice: choice() });
    } catch {
      report({
        type: "dictationchoice",
        choice: choice(),
        error: m(msg("Could not save settings.")),
      });
    }
  }
  function insert(operation, draft) {
    if (signal.aborted || completed?.operation !== operation) return;
    const { captured, transcript, locale } = completed;
    const result = resolveTranscriptCommit(
      captured,
      { ...draft, ownerKey: captured.ownerKey },
      transcript,
      locale,
    );
    if (result.kind !== "commit") {
      completed = undefined;
      report({
        type: "dictation",
        operation,
        phase: "error",
        message: m(msg("No speech was detected.")),
      });
      return;
    }
    // The shared resolver only inserts when the draft changed during recording.
    const start =
      draft.text === captured.text
        ? captured.selection.start
        : result.selection.end - (result.text.length - draft.text.length);
    const end = draft.text === captured.text ? captured.selection.end : start;
    report({
      type: "dictation",
      operation,
      phase: "transcript",
      edit: {
        start: Buffer.byteLength(draft.text.slice(0, start)),
        end: Buffer.byteLength(draft.text.slice(0, end)),
        text: result.text.slice(start, result.selection.end),
      },
    });
  }
  async function start(operation, draft) {
    const id = grant();
    if (active || !available || !id || signal.aborted) {
      report({
        type: "dictation",
        operation,
        phase: "error",
        message: m(msg("Choose a transcription destination in Voice settings.")),
      });
      return;
    }
    const job = { controller: new AbortController(), operation, stop: undefined, finishing: false };
    completed = undefined;
    active = job;
    const until = AbortSignal.any([signal, job.controller.signal]);
    const canSend = () => !until.aborted && grant() === id && active === job;
    const emit = (phase, fields = {}) => {
      if (!until.aborted) report({ type: "dictation", operation, phase, ...fields });
    };
    let failure = m(msg("Could not prepare voice transcription."));
    try {
      const status = await Effect.runPromise(
        getEnvironmentSpeechStatus(connection).pipe(Effect.provide(owner.http)),
        { signal: until },
      );
      if (!status.supported || status.state === "missing-model") {
        emit("error", {
          message: status.supported
            ? m(msg("Choose a transcription model in Voice settings."))
            : status.reason,
        });
        return;
      }
      if (!canSend()) throw new Error("Destination changed");
      const options = settings();
      const locale =
        options.speechTranslateToEnglish && status.supportsTranslation
          ? "en"
          : status.languages
            ? effectiveSpeechLanguage(status, options.speechLanguage)
            : "auto";
      failure = m(msg("Could not start the microphone."));
      let previousMeter;
      const pcm = await capture(
        process.env.T3CODE_RUNE_EXECUTABLE,
        until,
        (stop) => {
          job.stop = stop;
          emit("recording");
          if (job.finishing) stop();
        },
        (frames, level) => {
          if (!job.stop) return;
          const seconds = Math.floor(frames / 16000);
          const meter = seconds * 101 + level;
          if (meter !== previousMeter) emit("recording", { seconds, level });
          previousMeter = meter;
        },
      );
      failure = m(msg("Voice transcription failed. Check the destination in Settings → Voice."));
      if (!canSend()) throw new Error("Destination changed");
      emit("transcribing");
      const result = await Effect.runPromise(
        transcribeEnvironmentPcm(connection, pcm, options, canSend).pipe(
          Effect.provide(owner.http),
        ),
        { signal: until },
      );
      if (!canSend()) throw new Error("Destination changed");
      completed = {
        operation,
        captured: { ...draft, ownerKey: String(operation) },
        transcript: result.text,
        locale,
      };
      insert(operation, draft);
    } catch {
      emit("error", { message: failure });
    } finally {
      if (active === job) active = undefined;
    }
  }
  function finish(operation, cancel) {
    if (cancel && completed?.operation === operation) completed = undefined;
    if (active?.operation !== operation) return;
    if (cancel) {
      active.controller.abort();
      active = undefined;
    } else if (!active.finishing) {
      active.finishing = true;
      active.stop?.();
    }
  }
  return { choice, select, start, insert, finish };
}
