import { expect, it } from "vite-plus/test";
import { savedViewStore } from "./savedViews.ts";

it("keeps each list's views apart and refuses malformed views", () => {
  let stored: string | undefined = '{"issues":[{"id":"a","name":"Mine","preferences":{}}]}';
  const store = savedViewStore(
    () => stored,
    (value) => {
      stored = value;
    },
  );
  const view = { id: "b", name: "Reviewing", preferences: { involvement: 1 } };

  expect(store.write("pullRequests", [view])).toBe(true);
  expect(store.load()).toEqual({
    pullRequests: [view],
    issues: [{ id: "a", name: "Mine", preferences: {} }],
  });

  expect(store.write("pullRequests", [{ ...view, name: " " }])).toBe(false);
  expect(store.write("threads", [view])).toBe(false);
  expect(store.load().pullRequests).toEqual([view]);

  stored = "not json";
  expect(store.load()).toEqual({ pullRequests: [], issues: [] });
});
