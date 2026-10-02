import type { Accent, ThemeMode } from "@/lib/api/types";

/**
 * The palette, as the app talks about it.
 *
 * The colours themselves live in `app/design-system.css` — this module only
 * names the choices and says which attribute carries them, so there is one
 * place that knows the stylesheet's contract and no component has to.
 */

/** What the account can choose, in the order the controls offer it. */
export const THEME_MODES: ReadonlyArray<{ value: ThemeMode; label: string }> = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "System" },
];

/**
 * The accent hues, with the swatch each one shows in the picker.
 *
 * Two swatches per hue because the base accent differs by ground: on dark it
 * is the light ramp's `-400` step, which is what lifts it clear of the page.
 * A picker that showed only the light base would be describing a palette the
 * viewer is not currently looking at.
 */
export const ACCENTS: ReadonlyArray<{
  value: Accent;
  label: string;
  light: string;
  dark: string;
}> = [
  { value: "blue", label: "Blue", light: "#3570bd", dark: "#8ab7f4" },
  { value: "green", label: "Green", light: "#028653", dark: "#7bc89c" },
  { value: "purple", label: "Purple", light: "#8359ae", dark: "#c3a4e8" },
];

/** What the appearance controls hold, and what gets stamped on `<html>`. */
export interface Appearance {
  mode: ThemeMode;
  accent: Accent;
}

export const DEFAULT_APPEARANCE: Appearance = { mode: "system", accent: "blue" };

/** The key the pre-paint script in `app/layout.tsx` reads. Keep them in step. */
export const APPEARANCE_KEY = "finplan.appearance";

/**
 * Resolve `system` against the machine.
 *
 * The stylesheet has no `prefers-color-scheme` branch by design: resolving
 * here means the dark mapping is written once rather than duplicated for the
 * media query, and it is the only way the preview below a "System" control can
 * show what the viewer will actually get.
 */
export function resolveMode(mode: ThemeMode): "light" | "dark" {
  if (mode !== "system") return mode;
  if (typeof window === "undefined") return "light";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/**
 * Stamp a palette onto an element — `<html>` for the app, a preview box for
 * the picker. Both read the same tokens, so the preview cannot drift from the
 * thing it previews.
 */
export function applyAppearance(el: HTMLElement, appearance: Appearance): void {
  el.dataset.theme = resolveMode(appearance.mode);
  el.dataset.accent = appearance.accent;
}

/** Narrow whatever came back from storage; anything else is the default. */
export function parseAppearance(raw: unknown): Appearance {
  if (typeof raw !== "object" || raw === null) return DEFAULT_APPEARANCE;
  const held = raw as Record<string, unknown>;
  const mode = THEME_MODES.find((m) => m.value === held.mode)?.value;
  const accent = ACCENTS.find((a) => a.value === held.accent)?.value;
  return {
    mode: mode ?? DEFAULT_APPEARANCE.mode,
    accent: accent ?? DEFAULT_APPEARANCE.accent,
  };
}

// ── Dark style (this device) ─────────────────────────────────────────────

/**
 * How a dark ground is layered: Charcoal lays a lighter sheet on a darker
 * desk, the way light mode does; Midnight inverts it, a near-black sheet on a
 * lifted desk. A per-device preference — kept in local storage, not on the
 * account — because it is about the screen in front of you.
 */
export type DarkStyle = "charcoal" | "midnight";

export const DARK_STYLES: ReadonlyArray<{ value: DarkStyle; label: string }> = [
  { value: "charcoal", label: "Charcoal" },
  { value: "midnight", label: "Midnight" },
];

/** Read by the pre-paint script in `app/layout.tsx` too. Keep them in step. */
export const DARK_STYLE_KEY = "finplan.darkStyle";

export function parseDarkStyle(raw: unknown): DarkStyle {
  return raw === "midnight" ? "midnight" : "charcoal";
}

/** Stamp the style as `data-ground`; the stylesheet only reads it on dark. */
export function applyDarkStyle(el: HTMLElement, style: DarkStyle): void {
  if (style === "midnight") el.dataset.ground = "midnight";
  else delete el.dataset.ground;
}
