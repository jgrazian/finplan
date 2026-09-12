"use client";

import { useEffect, useState } from "react";
import { SegmentedControl } from "@/components/ui";
import { ACCENTS, type Appearance, applyAppearance, THEME_MODES, useResolvedMode } from "@/lib/theme";
import { PanelNote } from "./chrome";

const LABEL = "color-mix(in srgb, var(--color-text) 70%, transparent)";

/**
 * Mode, accent, and a preview of the two together.
 *
 * The preview is a real fragment of the app's chrome — a selected tab, a quiet
 * one, a tag — inside an element carrying the candidate palette's `data-theme`
 * and `data-accent`. Because the tokens cascade, it is the app's own CSS
 * drawing it, so it cannot show a palette the app would not produce. It also
 * lets the choice be seen before it is saved, without the whole page flipping
 * under a decision that has not been made yet.
 */
export function AppearanceFields({
  value,
  onChange,
  readOnly,
}: {
  value: Appearance;
  onChange: (appearance: Appearance) => void;
  readOnly?: boolean;
}) {
  const resolved = useResolvedMode(value.mode);

  return (
    <>
      <div style={{ display: "flex", gap: 30, alignItems: "flex-start", flexWrap: "wrap" }}>
        <div>
          <div style={{ fontSize: 12, marginBottom: 5, color: LABEL }}>Mode</div>
          <SegmentedControl
            ariaLabel="Appearance mode"
            options={THEME_MODES.map((mode) => ({
              value: mode.value,
              label: mode.label,
              disabled: readOnly,
            }))}
            value={value.mode}
            onChange={(mode) => onChange({ ...value, mode })}
          />
        </div>

        <div>
          <div style={{ fontSize: 12, marginBottom: 5, color: LABEL }}>Accent</div>
          {/* A radiogroup rather than three loose buttons: they are one
              choice, and a screen reader should hear it as one. */}
          <div style={{ display: "flex", gap: 8 }} role="radiogroup" aria-label="Accent">
            {ACCENTS.map((hue) => {
              const on = hue.value === value.accent;
              return (
                <button
                  key={hue.value}
                  type="button"
                  role="radio"
                  aria-checked={on}
                  disabled={readOnly}
                  className="btn"
                  style={{
                    fontSize: 13,
                    padding: "5px 11px",
                    gap: 7,
                    ...(on
                      ? {
                          borderColor: "var(--color-accent)",
                          background: "color-mix(in srgb, var(--color-accent) 12%, transparent)",
                        }
                      : null),
                  }}
                  onClick={() => onChange({ ...value, accent: hue.value })}
                >
                  <span
                    aria-hidden
                    style={{
                      width: 11,
                      height: 11,
                      flex: "none",
                      display: "block",
                      // The base accent of the ground being chosen for, not
                      // always the light one: on dark it is the light ramp's
                      // 400 step, and a swatch showing the other would be
                      // describing a palette the viewer is not about to get.
                      background: resolved === "dark" ? hue.dark : hue.light,
                    }}
                  />
                  {hue.label}
                </button>
              );
            })}
          </div>
        </div>

        <div>
          <div style={{ fontSize: 12, marginBottom: 5, color: LABEL }}>Preview</div>
          <Preview appearance={value} resolved={resolved} />
        </div>
      </div>

      <PanelNote>
        {value.mode === "system" && `Following the operating system — ${resolved} right now. `}
        Ground, ink and dividers are shared by all six sets; only the accent
        ramp turns, and every step of it keeps the same visual weight whichever
        hue is chosen. The palette belongs to the account, not to a scenario,
        so every plan is drawn in it.
      </PanelNote>
    </>
  );
}

function Preview({
  appearance,
  resolved,
}: {
  appearance: Appearance;
  resolved: "light" | "dark";
}) {
  const [box, setBox] = useState<HTMLDivElement | null>(null);

  // Stamped onto the node rather than rendered into JSX, so the `system`
  // reading — which only exists in the browser — is never guessed on the
  // server. `resolved` is a dependency because the machine can change its
  // mind while this is on screen.
  useEffect(() => {
    if (box) applyAppearance(box, appearance);
  }, [box, appearance, resolved]);

  const TAB: React.CSSProperties = {
    fontFamily: "var(--font-heading)",
    fontWeight: 600,
    fontSize: 11,
    letterSpacing: ".06em",
    textTransform: "uppercase",
    padding: "4px 9px",
  };

  return (
    <div ref={setBox}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "8px 10px",
          border: "1px solid var(--color-divider)",
          background: "var(--color-bg)",
          color: "var(--color-text)",
        }}
      >
        <span style={{ ...TAB, background: "var(--color-accent)", color: "var(--color-bg)" }}>
          Results
        </span>
        <span style={{ ...TAB, color: "color-mix(in srgb, var(--color-text) 50%, transparent)" }}>
          Plan
        </span>
        <span className="tag tag-accent">91.4%</span>
      </div>
    </div>
  );
}
