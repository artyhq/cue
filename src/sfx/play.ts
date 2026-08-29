import { createUISFX, type CueName } from "uisfx";
import { SFX_CUES, SFX_PACK, SFX_PREF_KEY, SFX_VOLUME, type SfxId } from "./catalog";

const PILL_CUES = Object.values(SFX_CUES) as CueName[];

const player = createUISFX({
  pack: SFX_PACK,
  volume: 0.7,
  preferences: { key: SFX_PREF_KEY },
});

export function playSfx(id: SfxId): Promise<void> {
  return playSfxAsync(id);
}

async function playSfxAsync(id: SfxId): Promise<void> {
  const cue = SFX_CUES[id];
  const volume = SFX_VOLUME[id];
  const options = volume === undefined ? undefined : { volume };
  let handle = player.play(cue, options);
  if (!handle) {
    const unlocked = await player.unlock();
    if (unlocked) handle = player.play(cue, options);
  }
  if (!handle) {
    // NOTE: skipped — muted, locked, or cue unavailable. Caller starts capture immediately.
    return;
  }
  await Promise.race([
    handle.ended,
    new Promise<void>((resolve) => {
      window.setTimeout(resolve, 1500);
    }),
  ]);
}

export function preloadSfx(): void {
  void player.preload(PILL_CUES);
}

export function unlockSfx(): void {
  void player.unlock();
}

export function isSfxEnabled(): boolean {
  return player.isEnabled();
}

export function setSfxEnabled(enabled: boolean): void {
  if (!enabled) player.stopAll();
  player.setEnabled(enabled);
}

export function attachSfxUnlock(): () => void {
  const unlock = () => {
    void player.unlock();
  };
  window.addEventListener("pointerdown", unlock);
  window.addEventListener("keydown", unlock);
  return () => {
    window.removeEventListener("pointerdown", unlock);
    window.removeEventListener("keydown", unlock);
  };
}
