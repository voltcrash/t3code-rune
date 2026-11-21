import * as Effect from "effect/Effect";
import * as ElectronApp from "../electron/ElectronApp.ts";
import { selectLanguage } from "./catalog.ts";

export { m, onLocaleChange } from "./catalog.ts";

/** Activates the saved client language using Electron's preferred system languages. */
export const applyLanguage = Effect.fn("desktop.i18n.applyLanguage")(function* (language: string) {
  const app = yield* ElectronApp.ElectronApp;
  const preferred = yield* app.preferredSystemLanguages;
  yield* Effect.promise(() => selectLanguage(language, preferred));
});
