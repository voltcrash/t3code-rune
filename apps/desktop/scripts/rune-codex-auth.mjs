import { receiveCodexAuthCallback } from "@t3tools/shared/codexAuthCallback";
import { codexAuthDeliveryUrl, readCodexAuthHandoff } from "@t3tools/shared/codexAuthHandoff";
import { openExternalUrl } from "./rune-external.mjs";

export function handleCodexAuthLink(url, development, signal) {
  const request = readCodexAuthHandoff(url, development);
  if (!request) return;
  return receiveCodexAuthCallback(
    request.authorizationUrl,
    openExternalUrl,
    (callback) => codexAuthDeliveryUrl(request, callback),
    signal,
  ).catch(() => {
    if (!signal.aborted) process.stderr.write("Could not complete ChatGPT desktop sign-in.\n");
  });
}
