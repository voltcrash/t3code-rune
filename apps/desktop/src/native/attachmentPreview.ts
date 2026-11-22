import { readFilePreviewResponse } from "@t3tools/client-runtime/file-preview";
import { FILE_TEXT_PREVIEW_MAX_BYTES } from "@t3tools/shared/filePreview";
import * as Effect from "effect/Effect";
import * as Option from "effect/Option";
import { HttpClientResponse } from "effect/http";
import { AttachmentUnavailable } from "./attachments.ts";
import { attachmentResponse } from "./downloadAttachment.ts";

export const readAttachmentPreview = Effect.fn("nativeDesktop.readAttachmentPreview")(function* (
  baseUrl: string,
  relativeUrl: string,
  size: number,
) {
  const range =
    size > FILE_TEXT_PREVIEW_MAX_BYTES ? `bytes=0-${FILE_TEXT_PREVIEW_MAX_BYTES}` : undefined;
  const response = yield* attachmentResponse(baseUrl, relativeUrl, range);
  const web = HttpClientResponse.toWeb(response);
  if (Option.isNone(web))
    return yield* new AttachmentUnavailable({ message: "The attachment is unavailable." });
  return yield* Effect.tryPromise({
    try: (signal) => readFilePreviewResponse(web.value, signal),
    catch: (cause) =>
      new AttachmentUnavailable({
        message: cause instanceof Error ? cause.message : "The attachment is unavailable.",
      }),
  });
}, Effect.scoped);
