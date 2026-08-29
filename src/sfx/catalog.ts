import type { CueName } from "uisfx";

/**
 * Pill-mode earcons via uisfx semantic cues.
 * Settings never plays these — overlay only.
 *
 * Pack: `minimal` (dry, precise, Apple/SaaS). No `recording` loop while
 * capture is live: that would leak into the mic / system mix.
 */
export type SfxId =
  | "recordStart"
  | "recordStop"
  | "recordCancel"
  | "dismiss"
  | "openFolder"
  | "playFile"
  | "error"
  | "timerWarn";

export const SFX_PACK = "minimal" as const;
export const SFX_PREF_KEY = "cue:sound";

export const SFX_CUES: Record<SfxId, CueName> = {
  recordStart: "start",
  recordStop: "success",
  recordCancel: "cancel",
  dismiss: "close",
  openFolder: "open",
  playFile: "play",
  error: "error",
  timerWarn: "warning",
};

export const SFX_VOLUME: Partial<Record<SfxId, number>> = {
  timerWarn: 0.28,
};
