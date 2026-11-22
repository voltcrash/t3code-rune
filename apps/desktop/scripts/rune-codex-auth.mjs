import { receiveCodexAuthCallback } from "@t3tools/shared/codexAuthCallback";
import { codexAuthDeliveryUrl, readCodexAuthHandoff } from "@t3tools/shared/codexAuthHandoff";
import * as HostProcess from "@t3tools/shared/HostProcess";
import * as Effect from "effect/Effect";
import * as NodeChildProcess from "node:child_process";
import * as NodePath from "node:path";

function openBrowser(url) {
  const platform = Effect.runSync(HostProcess.Platform);
  let command = "xdg-open";
  let args = [url];
  if (platform === "darwin") command = "/usr/bin/open";
  if (platform === "win32") {
    const systemRoot = process.env.SystemRoot ?? process.env.windir ?? "C:\\Windows";
    command = NodePath.join(systemRoot, "System32", "rundll32.exe");
    args = ["url.dll,FileProtocolHandler", url];
  }
  return new Promise((resolve) => {
    const child = NodeChildProcess.spawn(command, args, { stdio: "ignore", windowsHide: true });
    child.once("error", () => resolve(false));
    child.once("exit", (code) => resolve(code === 0));
    child.unref();
  });
}

export function handleCodexAuthLink(url, development, signal) {
  const request = readCodexAuthHandoff(url, development);
  if (!request) return;
  return receiveCodexAuthCallback(
    request.authorizationUrl,
    openBrowser,
    (callback) => codexAuthDeliveryUrl(request, callback),
    signal,
  ).catch(() => {
    if (!signal.aborted) process.stderr.write("Could not complete ChatGPT desktop sign-in.\n");
  });
}
