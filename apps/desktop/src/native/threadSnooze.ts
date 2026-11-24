import {
  pickSnoozePreset,
  resolveSnoozePresets,
  type SnoozePreset,
} from "@t3tools/client-runtime/state/thread-settled";
import type { SnoozePresetSetting } from "@t3tools/contracts/settings";
import { m } from "../i18n/catalog.ts";

/**
 * The snooze submenu for one thread. Like the web menu, the choices resolve when the menu opens
 * and a pick re-resolves a duration so it starts at selection.
 */
export function threadSnoozeMenu() {
  let shown:
    | {
        readonly thread: string;
        readonly presets: ReadonlyArray<SnoozePreset>;
        readonly settings: ReadonlyArray<SnoozePresetSetting> | undefined;
      }
    | undefined;
  return {
    open(thread: string, settings: ReadonlyArray<SnoozePresetSetting> | undefined, now: Date) {
      const presets = resolveSnoozePresets(now, settings ? { presets: settings } : {}, m);
      shown = { thread, presets, settings };
      return presets.map((preset) => ({
        id: preset.id,
        label: preset.label,
        when: preset.whenLabel,
      }));
    },
    /** The wake time for a shown choice, or undefined when it was not shown or has passed. */
    pick(thread: string, id: string, now: Date): string | undefined {
      if (shown?.thread !== thread) return undefined;
      return pickSnoozePreset({
        shown: shown.presets,
        id,
        now,
        ...(shown.settings ? { presets: shown.settings } : {}),
        resolve: m,
      })?.snoozedUntil;
    },
  };
}
