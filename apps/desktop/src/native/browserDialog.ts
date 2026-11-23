import type { PreviewStreamControl } from "@t3tools/client-runtime/preview/server-browser-stream";

type Dialog = NonNullable<PreviewStreamControl["dialog"]>;

export class BrowserDialog {
  private id = 0;
  private state: "open" | "sent" | "check" = "open";
  private control: PreviewStreamControl | null = null;

  update(next: PreviewStreamControl | null) {
    const before = this.control?.dialog;
    const after = next?.dialog;
    if (
      before?.type !== after?.type ||
      before?.message !== after?.message ||
      before?.defaultValue !== after?.defaultValue
    ) {
      this.state = "open";
      this.id++;
    } else {
      if (this.control?.generation !== next?.generation) this.id++;
      // A compacted stream can replace one prompt with another bearing the same text.
      if (after && this.state === "sent") this.state = "check";
    }
    this.control = next;
  }

  get current() {
    const dialog = this.control?.dialog;
    return dialog && this.state !== "sent"
      ? { ...dialog, id: this.id, recheck: this.state === "check" }
      : null;
  }

  rechecked(id: number, observed: PreviewStreamControl["dialog"]) {
    if (this.id !== id || this.state !== "check" || this.control?.controller !== "you") return;
    this.update({ ...this.control, dialog: observed });
    this.id++;
    this.state = "open";
  }

  answer(id: number, send: (dialog: Dialog) => boolean) {
    if (
      this.control?.controller === "you" &&
      this.control.dialog &&
      this.state === "open" &&
      id === this.id
    ) {
      if (send(this.control.dialog)) this.state = "sent";
    }
  }
}
