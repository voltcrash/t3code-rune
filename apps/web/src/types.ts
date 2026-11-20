import type {
  ChatAttachment,
  ChatImageAttachment,
  ChatFileAttachment,
} from "@t3tools/client-runtime/work-log/timeline";
import { imageMimeType } from "@t3tools/shared/image";
import type {
  ProjectScript as ContractProjectScript,
  ProviderInteractionMode,
  RuntimeMode,
} from "@t3tools/contracts";
import type {
  EnvironmentProject,
  EnvironmentThreadShell,
  ThreadRuntimeSummary,
} from "@t3tools/client-runtime/state/shell";
import type { ThreadCheckpointSummary } from "@t3tools/client-runtime/state/thread-checkpoints";
import { videoMimeType } from "@t3tools/shared/video";

export { videoMimeType } from "@t3tools/shared/video";

export type SessionPhase = "disconnected" | "connecting" | "ready" | "running";
export const DEFAULT_RUNTIME_MODE: RuntimeMode = "full-access";

export const DEFAULT_INTERACTION_MODE: ProviderInteractionMode = "default";
export const DEFAULT_THREAD_TERMINAL_HEIGHT = 280;
export const DEFAULT_THREAD_TERMINAL_ID = "term-1";
export const MAX_TERMINALS_PER_GROUP = 4;
export type ProjectScript = ContractProjectScript;

export interface ThreadTerminalGroup {
  id: string;
  terminalIds: string[];
  splitDirection?: "horizontal" | "vertical";
}

export type {
  ChatAttachment,
  ChatImageAttachment,
  ChatFileAttachment,
  ChatUnknownAttachment,
  ChatMessage,
  ProposedPlan,
} from "@t3tools/client-runtime/work-log/timeline";

export function isImageAttachment(attachment: ChatAttachment): attachment is ChatImageAttachment {
  // Legacy file attachments can be images; unknown types must never be reclassified by name.
  if (attachment.type === "image") return true;
  return attachment.type === "file" && imageMimeType(attachment) !== null;
}

export function isFileAttachment(attachment: ChatAttachment): attachment is ChatFileAttachment {
  // Keep disjoint from isImageAttachment so independently filtered sets do not render legacy images twice.
  return attachment.type === "file" && !isImageAttachment(attachment);
}

export function isVideoAttachment(attachment: ChatFileAttachment): boolean {
  return videoMimeType(attachment) !== null;
}

export type TurnDiffFileChange = ThreadCheckpointSummary["files"][number];
export type TurnDiffSummary = ThreadCheckpointSummary;

export type Project = EnvironmentProject;
export type Thread = EnvironmentThreadShell;
export type ThreadShell = EnvironmentThreadShell;

export type SidebarThreadSummary = EnvironmentThreadShell;
export type ThreadSession = ThreadRuntimeSummary;
