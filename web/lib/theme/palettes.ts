/**
 * The palette, as the app talks about it.
 *
 * The colours themselves live in `app/design-system.css` — this module only
 * names the choices and says which attribute carries them, so there is one
 * place that knows the stylesheet's contract and no component has to.
 *
 * Both choices are the device's, not the account's: they are about the screen
 * in front of you, so they live in local storage and apply the moment they
 * are picked. There is one accent, the Ledger blue; nothing chooses it.
 */

// ── Mode (this device) ───────────────────────────────────────────────────

export type ThemeMode = "light" | "dark" | "system";

/** What the controls offer, in order. */
export const THEME_MODES: ReadonlyArray<{ value: ThemeMode; label: string }> = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "System" },
];

/**
 * Read by the pre-paint script in `app/layout.tsx` too. Keep them in step.
 *
 * Holds `{ "mode": … }` — the same key and shape the account's appearance was
 * cached under before it moved here, so a device keeps the mode it last had.
 */
export const APPEARANCE_KEY = "finplan.appearance";

/** Narrow whatever came back from storage; anything else is `system`. */
export function parseThemeMode(raw: string | null): ThemeMode {
  try {
    const held = JSON.parse(raw ?? "{}") as { mode?: unknown };
    return THEME_MODES.find((m) => m.value === held?.mode)?.value ?? "system";
  } catch {
    return "system";
  }
}

export function serializeThemeMode(mode: ThemeMode): string {
  return JSON.stringify({ mode });
}

/**
 * Resolve `system` against the machine.
 *
 * The stylesheet has no `prefers-color-scheme` branch by design: resolving
 * here means the dark mapping is written once rather than duplicated for the
 * media query.
 */
export function resolveMode(mode: ThemeMode): "light" | "dark" {
  if (mode !== "system") return mode;
  if (typeof window === "undefined") return "light";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/** Stamp the resolved ground as `data-theme`, all the stylesheet reads. */
export function applyThemeMode(el: HTMLElement, mode: ThemeMode): void {
  el.dataset.theme = resolveMode(mode);
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
