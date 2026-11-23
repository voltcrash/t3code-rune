import type { PreviewStreamControl } from "@t3tools/client-runtime/preview/server-browser-stream";

type Dialog = NonNullable<PreviewStreamControl["dialog"]>;

export class BrowserDialog {
  private id = 0;
  private answered = false;
  private control: PreviewStreamControl | null = null;

  update(next: PreviewStreamControl | null) {
    const before = this.control?.dialog;
    const after = next?.dialog;
    if (
      before?.type !== after?.type ||
      before?.message !== after?.message ||
      before?.defaultValue !== after?.defaultValue
    ) {
      this.answered = false;
      this.id++;
    } else if (this.control?.generation !== next?.generation) {
      this.id++;
    }
    this.control = next;
  }

  get current() {
    const dialog = this.control?.dialog;
    return dialog && !this.answered ? { ...dialog, id: this.id } : null;
  }

  answer(id: number, send: (dialog: Dialog) => boolean) {
    if (
      this.control?.controller === "you" &&
      this.control.dialog &&
      !this.answered &&
      id === this.id
    ) {
      this.answered = send(this.control.dialog);
    }
  }
}
