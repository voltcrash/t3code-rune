import type { PreviewStreamControl } from "@t3tools/client-runtime/preview/server-browser-stream";
import { expect, it, vi } from "vite-plus/test";
import { BrowserDialog } from "./browserDialog.ts";

it("sends one answer per observed dialog even when another viewer joins", () => {
  const dialogs = new BrowserDialog();
  const control: PreviewStreamControl = {
    canOperate: true,
    controller: "you",
    generation: 2,
    dialog: { type: "confirm", message: "Continue?", defaultValue: "" },
  };
  const send = vi.fn(() => true);
  dialogs.update(control);
  const first = dialogs.current!.id;
  dialogs.answer(first, send);
  dialogs.update({ ...control });
  dialogs.answer(first, send);
  expect(dialogs.current?.recheck).toBe(true);
  dialogs.update({ ...control, controller: "another-viewer", generation: 3 });
  dialogs.update({ ...control, generation: 4 });
  expect(dialogs.current?.recheck).toBe(true);
  expect(send).toHaveBeenCalledTimes(1);

  const ambiguous = dialogs.current!.id;
  dialogs.rechecked(first, control.dialog);
  expect(dialogs.current?.recheck).toBe(true);
  dialogs.rechecked(ambiguous, control.dialog);
  dialogs.answer(ambiguous, send);
  expect(send).toHaveBeenCalledTimes(1);
  expect(dialogs.current?.recheck).toBe(false);
  dialogs.answer(dialogs.current!.id, send);
  expect(send).toHaveBeenCalledTimes(2);
  dialogs.update({ ...control, dialog: null });
  dialogs.update(control);
  const successor = dialogs.current!.id;
  expect(dialogs.current?.recheck).toBe(false);
  dialogs.answer(first, send);
  expect(send).toHaveBeenCalledTimes(2);
  dialogs.answer(successor, send);
  expect(send).toHaveBeenCalledTimes(3);
});
