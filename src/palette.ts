/**
 * Cue color scales.
 *
 * Neutral (sumi-ink) + 4 semantic scales, 12 steps, light / dark.
 * Spec: ./temp/cue-palette.html
 *
 * Step roles (Radix-style, same for every scale):
 *  1–2  app / subtle background
 *  3–5  UI background (default / hover / active)
 *  6–8  borders (subtle / default / hover)
 *  9–10 solid fill (default / hover)
 * 11–12 text (low / high contrast)
 *
 * Error & warning carry higher chroma so they interrupt.
 * Success & WoV/listening stay quieter so they don't compete.
 * Warning shares the warm ink hue with neutral — chroma, not hue, is the signal.
 */

export const SCALE_NAMES = ["neutral", "error", "warning", "success", "wov"] as const;
export type ScaleName = (typeof SCALE_NAMES)[number];
export type ThemeMode = "light" | "dark";
export type ScaleStep = 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12;

export const SCALE_ROLES = [
  "app-bg",
  "subtle-bg",
  "ui-bg",
  "ui-hover",
  "ui-active",
  "border-subtle",
  "border",
  "border-hover",
  "solid",
  "solid-hover",
  "text-low",
  "text-high",
] as const;

export const SCALE_DEFS: Record<
  ScaleName,
  { hueL: number; hueD: number; satMax: number; satMaxD: number }
> = {
  neutral: { hueL: 40, hueD: 38, satMax: 11, satMaxD: 16 },
  error: { hueL: 9, hueD: 9, satMax: 62, satMaxD: 58 },
  warning: { hueL: 42, hueD: 44, satMax: 70, satMaxD: 62 },
  success: { hueL: 148, hueD: 150, satMax: 40, satMaxD: 38 },
  wov: { hueL: 218, hueD: 220, satMax: 42, satMaxD: 40 },
};

const LIGHT_L = [99, 97.4, 95, 92, 88.3, 84, 77.3, 67.5, 54, 47, 37, 16];
const LIGHT_S = [0.12, 0.22, 0.32, 0.42, 0.52, 0.62, 0.72, 0.85, 1.0, 0.94, 0.82, 0.5];
const DARK_L = [9.5, 12.5, 16, 19.5, 23.5, 29, 36, 45.5, 55, 60.5, 72, 95];
const DARK_S = [0.42, 0.46, 0.5, 0.54, 0.58, 0.62, 0.66, 0.76, 1.0, 0.94, 0.78, 0.42];

function hslToHex(h: number, s: number, l: number): string {
  s /= 100;
  l /= 100;
  const k = (n: number) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) =>
    l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  const toHex = (x: number) =>
    Math.round(255 * x)
      .toString(16)
      .padStart(2, "0");
  return `#${toHex(f(0))}${toHex(f(8))}${toHex(f(4))}`;
}

export type Swatch = { hex: string; hsl: string };

export function buildScale(name: ScaleName, theme: ThemeMode): Swatch[] {
  const def = SCALE_DEFS[name];
  const hue = theme === "light" ? def.hueL : def.hueD;
  const satMax = theme === "light" ? def.satMax : def.satMaxD;
  const L = theme === "light" ? LIGHT_L : DARK_L;
  const S = theme === "light" ? LIGHT_S : DARK_S;
  return L.map((l, i) => {
    const s = satMax * S[i];
    return {
      hsl: `hsl(${hue} ${s.toFixed(1)}% ${l}%)`,
      hex: hslToHex(hue, s, l).toUpperCase(),
    };
  });
}

export function swatch(name: ScaleName, step: ScaleStep, theme: ThemeMode): Swatch {
  return buildScale(name, theme)[step - 1];
}

function scaleDecls(theme: ThemeMode): string {
  return SCALE_NAMES.flatMap((name) =>
    buildScale(name, theme).map((s, i) => `  --${name}-${i + 1}: ${s.hex};`),
  ).join("\n");
}

/** Primitive scale variables. Semantic aliases live in theme.css. */
export function paletteCss(): string {
  return `/* Generated from src/palette.ts — do not edit by hand. */
:root {
${scaleDecls("light")}
}
@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
${scaleDecls("dark")}
  }
}
:root[data-theme="dark"] {
${scaleDecls("dark")}
}
:root[data-theme="light"] {
${scaleDecls("light")}
}
`;
}
