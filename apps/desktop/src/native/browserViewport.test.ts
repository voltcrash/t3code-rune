import { expect, it } from "vite-plus/test";
import { FILL_PREVIEW_VIEWPORT as fill } from "@t3tools/contracts";
import { deviceViewport } from "./browserViewport.ts";

it("opens with the preferred viewport and preserves its identity when rotating", () => {
  const show = { kind: "show", width: 700, height: 520 } as const;
  const defaultSize = { _tag: "preset", presetId: "iphone-se", width: 375, height: 667 } as const;
  expect(deviceViewport(show, fill, defaultSize, 1.25)).toEqual(defaultSize);
  expect(deviceViewport(show, fill, fill, 1.25)).toMatchObject({
    _tag: "freeform",
    width: 544,
    height: 378,
  });
  expect(deviceViewport({ kind: "rotate" }, defaultSize, fill, 1)).toEqual({
    ...defaultSize,
    width: 667,
    height: 375,
  });
  expect(deviceViewport({ kind: "preset", id: "ipad-air" }, defaultSize, fill, 1)).toEqual({
    _tag: "preset",
    presetId: "ipad-air",
    width: 820,
    height: 1180,
  });
  expect(deviceViewport({ kind: "fill" }, defaultSize, fill, 1)).toEqual(fill);
});

it("validates freeform edits and rotates the edited size", () => {
  const size = { _tag: "freeform", width: 600, height: 400 } as const;
  const edit = { width: 900, height: 400, axis: "width", locked: true } as const;
  expect(deviceViewport({ kind: "edit", edit }, size, fill, 1)).toEqual({
    ...size,
    width: 900,
    height: 600,
  });
  expect(deviceViewport({ kind: "rotate", edit }, size, fill, 1)).toEqual({
    ...size,
    width: 600,
    height: 900,
  });
  expect(deviceViewport({ kind: "edit", edit: { ...edit, width: 9000 } }, size, fill, 1)).toBe(
    size,
  );
  expect(
    deviceViewport(
      { kind: "edit", edit: { ...edit, width: 3800, height: 3800, locked: false } },
      size,
      fill,
      1,
    ),
  ).toBe(size);
});
