"use client";

import { SegmentedControl } from "@/components/ui";
import {
  DARK_STYLES,
  THEME_MODES,
  useDarkStyle,
  useResolvedMode,
  useThemeMode,
} from "@/lib/theme";
import { PanelNote } from "./chrome";

const LABEL = "color-mix(in srgb, var(--color-text) 70%, transparent)";

/**
 * Mode and dark style — both this device's, both applied the moment they are
 * picked. Neither is part of the panel's draft: there is nothing on the
 * account to save them to, and the page itself is the preview.
 */
export function AppearanceFields() {
  const [mode, setMode] = useThemeMode();
  const resolved = useResolvedMode(mode);
  const [darkStyle, setDarkStyle] = useDarkStyle();

  return (
    <>
      <div style={{ display: "flex", gap: 30, alignItems: "flex-start", flexWrap: "wrap" }}>
        <div>
          <div style={{ fontSize: 12, marginBottom: 5, color: LABEL }}>Mode</div>
          <SegmentedControl
            ariaLabel="Appearance mode"
            options={THEME_MODES.map((m) => ({ value: m.value, label: m.label }))}
            value={mode}
            onChange={setMode}
          />
        </div>

        <div>
          <div style={{ fontSize: 12, marginBottom: 5, color: LABEL }}>Dark style</div>
          <SegmentedControl
            ariaLabel="Dark style"
            options={DARK_STYLES.map((s) => ({ value: s.value, label: s.label }))}
            value={darkStyle}
            onChange={setDarkStyle}
          />
        </div>
      </div>

      <PanelNote>
        {mode === "system" && `Following the operating system — ${resolved} right now. `}
        Kept on this device and applied straight away, so a phone and a desktop can
        differ.
      </PanelNote>
    </>
  );
}
