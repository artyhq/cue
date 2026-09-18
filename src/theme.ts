import { paletteCss } from "./palette";
import "./theme.css";

export type { ScaleName, ScaleStep, ThemeMode } from "./palette";
export { SCALE_DEFS, SCALE_NAMES, SCALE_ROLES, buildScale, swatch } from "./palette";

export type ThemeName = "system" | "light" | "dark";

const PALETTE_STYLE_ID = "cue-palette";

export function installPalette() {
  if (typeof document === "undefined") return;
  let node = document.getElementById(PALETTE_STYLE_ID) as HTMLStyleElement | null;
  if (!node) {
    node = document.createElement("style");
    node.id = PALETTE_STYLE_ID;
    const parent = document.head ?? document.documentElement;
    parent.insertBefore(node, parent.firstChild);
  }
  node.textContent = paletteCss();
}

installPalette();

export function applyTheme(theme: string) {
  const root = document.documentElement;
  if (theme === "light" || theme === "dark") {
    root.setAttribute("data-theme", theme);
  } else {
    root.removeAttribute("data-theme");
  }
}
