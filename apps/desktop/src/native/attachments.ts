// @effect-diagnostics nodeBuiltinImport:off -- Node’s file-backed Blob detects changes to the selected file while streaming it.
import { PASTED_TEXT_ATTACHMENT_THRESHOLD_BYTES } from "@t3tools/client-runtime/text-paste";
import { makeEnvironmentHttpApiGroupClient } from "@t3tools/client-runtime/rpc";
import {
  clampFileAttachmentUploadBytes,
  fileAttachmentTooLargeMessage,
} from "@t3tools/client-runtime/state/attachments";
import {
  getProviderAttachmentLimitError,
  MessageId,
  PROVIDER_SEND_TURN_MAX_INPUT_CHARS,
  PROVIDER_SEND_TURN_MAX_IMAGE_BYTES,
  PROVIDER_SEND_TURN_SUPPORTED_IMAGE_MIME_TYPES,
  ThreadId,
  WS_METHODS,
  type AttachmentCreateUploadUrlInput,
  type ChatAttachment,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as Exit from "effect/Exit";
import { FetchHttpClient, HttpBody, HttpClient } from "effect/http";
import * as Schema from "effect/Schema";
import { lookup } from "mrmime";
import * as NodeFS from "node:fs";
import * as NodeFSP from "node:fs/promises";
import * as NodePath from "node:path";
import type { openEnvironmentRpc } from "./environmentRpc.ts";
import { headersFor, type Session } from "./projects.ts";

export class AttachmentUnavailable extends Schema.TaggedError<AttachmentUnavailable>()(
  "AttachmentUnavailable",
  { message: Schema.String },
) {}

type EnvironmentRpc = Effect.Success<ReturnType<typeof openEnvironmentRpc>>;

const unavailable = (message: string) => new AttachmentUnavailable({ message });

export type SelectedFile = string | { readonly path: string; readonly pasted: string };

export function textPasteLimits(session: Session) {
  return {
    threshold: PASTED_TEXT_ATTACHMENT_THRESHOLD_BYTES,
    input: PROVIDER_SEND_TURN_MAX_INPUT_CHARS,
    file: session.attachmentUploads ? clampFileAttachmentUploadBytes(session.maxFileBytes) : 0,
  };
}

const openSelection = Effect.fn("nativeDesktop.openSelection")(function* (
  session: Session,
  selection: SelectedFile,
) {
  const pasted = typeof selection !== "string";
  const path = pasted ? selection.path : selection;
  const name = pasted ? selection.pasted : NodePath.basename(path).trim();
  if (!name || name.length > 255)
    return yield* unavailable("Choose a file with a name of 1 to 255 characters.");
  const mimeType = pasted ? "text/plain" : (lookup(name) ?? "application/octet-stream");
  const imageType = PROVIDER_SEND_TURN_SUPPORTED_IMAGE_MIME_TYPES.find((type) => type === mimeType);
  const limit = imageType
    ? PROVIDER_SEND_TURN_MAX_IMAGE_BYTES
    : clampFileAttachmentUploadBytes(session.maxFileBytes);
  if (!imageType && (!session.attachmentUploads || !limit))
    return yield* unavailable("This environment accepts image attachments only.");
  const blob = yield* Effect.tryPromise({
    try: async () => {
      if (!(await NodeFSP.stat(path)).isFile()) throw new Error("Expected a regular file");
      return await NodeFS.openAsBlob(path);
    },
    catch: () => unavailable(`Couldn’t read “${name}”. Choose the file again.`),
  });
  if (!blob.size) return yield* unavailable(`“${name}” is empty.`);
  if (blob.size > limit) return yield* unavailable(fileAttachmentTooLargeMessage(name, limit));
  const metadata: AttachmentCreateUploadUrlInput = imageType
    ? { type: "image", name, mimeType: imageType, sizeBytes: blob.size }
    : { type: "file", name, mimeType, sizeBytes: blob.size };
  return { blob, metadata, pasted };
});

export const prepareAttachments = Effect.fn("nativeDesktop.prepareAttachments")(function* (
  session: Session,
  rpc: EnvironmentRpc,
  paths: readonly SelectedFile[] = [],
  thread: string,
  operation: string,
) {
  if (!paths.length) return [];
  if (!session.canOperate) return yield* unavailable("This connection cannot upload attachments.");
  const files = yield* Effect.forEach(paths, (path) => openSelection(session, path));
  const limitError = getProviderAttachmentLimitError(
    files.map(({ metadata }) => ({ ...metadata, type: metadata.type ?? "image" })),
  );
  if (limitError) return yield* unavailable(limitError);
  if (!session.attachmentUploads) {
    const batches = yield* Effect.forEach(
      files,
      Effect.fn("nativeDesktop.persistLegacyImage")(function* (
        { blob, metadata }: (typeof files)[number],
        index: number,
      ) {
        const dataUrl = yield* Effect.tryPromise({
          try: async () =>
            `data:${metadata.mimeType};base64,${Buffer.from(await blob.arrayBuffer()).toString("base64")}`,
          catch: () => unavailable(`Couldn’t read “${metadata.name}”. Choose the file again.`),
        });
        return (yield* rpc[WS_METHODS.assetsPersistChatAttachments]({
          threadId: ThreadId.make(thread),
          messageId: MessageId.make(`${operation}-${index}`),
          attachments: [{ ...metadata, type: "image", dataUrl }],
        })).attachments;
      }),
    );
    return batches.flat();
  }

  const minted: string[] = [];
  return yield* Effect.forEach(
    files,
    Effect.fn("nativeDesktop.uploadAttachment")(function* ({
      metadata,
      blob,
      pasted,
    }: (typeof files)[number]) {
      const slot = yield* rpc[WS_METHODS.attachmentsCreateUploadUrl](metadata);
      minted.push(slot.attachmentId);
      const client = HttpClient.filterStatusOk(yield* HttpClient.HttpClient);
      yield* client
        .post(new URL(slot.relativeUrl, session.httpBaseUrl), {
          body: HttpBody.raw(blob, { contentType: metadata.mimeType }),
        })
        .pipe(
          Effect.provideService(FetchHttpClient.RequestInit, { redirect: "error" }),
          Effect.mapError(() =>
            unavailable(
              `Couldn’t upload “${metadata.name}”. Check the file and connection, then retry.`,
            ),
          ),
        );
      const stored = { ...metadata, id: slot.attachmentId };
      if (pasted)
        return {
          ...stored,
          type: "file",
          source: { _tag: "pasted-text" },
        } satisfies ChatAttachment;
      return { ...stored, type: metadata.type ?? "image" } satisfies ChatAttachment;
    }),
  ).pipe(
    Effect.onExit((exit) =>
      Exit.isSuccess(exit)
        ? Effect.void
        : Effect.forEach(
            minted,
            (attachmentId) =>
              rpc[WS_METHODS.attachmentsDelete]({ attachmentId }).pipe(
                Effect.timeout("2 seconds"),
                Effect.ignore,
              ),
            { concurrency: 4, discard: true },
          ),
    ),
  );
});

export const retryAttachments = Effect.fn("nativeDesktop.retryAttachments")(function* (
  session: Session,
  rpc: EnvironmentRpc,
  cached: readonly ChatAttachment[],
  paths: readonly SelectedFile[],
  thread: string,
  operation: string,
  onMissingThread?: () => void,
) {
  if (!session.attachmentUploads || !cached.length) return cached;
  const available = yield* Effect.forEach(
    cached,
    ({ id }) =>
      rpc[WS_METHODS.assetsCreateUrl]({ resource: { _tag: "attachment", attachmentId: id } }).pipe(
        Effect.as(true),
        Effect.catchTags({ AssetAttachmentNotFoundError: () => Effect.succeed(false) }),
      ),
    { concurrency: 4 },
  );
  if (available.every(Boolean)) return cached;

  const committed = yield* committedAttachments(session, thread, operation, onMissingThread);
  if (committed) return committed;
  return yield* prepareAttachments(session, rpc, paths, thread, operation);
});

export const committedAttachments = Effect.fn("nativeDesktop.committedAttachments")(function* (
  session: Session,
  thread: string,
  operation: string,
  onMissingThread?: () => void,
) {
  // A lost receipt can outlive the pending upload. Replay with the thread's committed attachment IDs.
  const api = yield* makeEnvironmentHttpApiGroupClient(session.httpBaseUrl, "orchestration");
  const target = { headers: headersFor(session), params: { threadId: ThreadId.make(thread) } };
  const snapshot = yield* api
    .threadBoundedSnapshot({ ...target, query: { compactTurnItems: "1", dedupeBodies: "1" } })
    .pipe(
      Effect.catchTags({
        EnvironmentResourceNotFoundError: (e) =>
          e.reason === "thread_not_found" ? Effect.succeed(null) : Effect.fail(e),
      }),
    );
  if (snapshot === null) onMissingThread?.();
  const committed = snapshot?.projection.messages.find((m) => m.id === operation);
  if (committed) return committed.attachments;
  let cursor = snapshot?.hasMoreHistory ? snapshot.historyCursor : null;
  while (cursor) {
    const page = yield* api.threadHistoryPage({
      ...target,
      query: { cursor, view: "conversation" },
    });
    for (const { item, sourceThreadId } of page.items) {
      if (sourceThreadId === thread && item.type === "user_message" && item.messageId === operation)
        return item.attachments ?? [];
    }
    cursor = page.hasMoreHistory ? page.nextCursor : null;
  }
  return null;
});
