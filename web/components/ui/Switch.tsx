"use client";

/**
 * An on/off switch: a pill with a sliding knob, `role="switch"` so it reads
 * as one. `label` names it for assistive tech; the visible label sits beside.
 */
export function Switch({
  label,
  on,
  disabled,
  onToggle,
}: {
  label: string;
  on: boolean;
  disabled?: boolean;
  onToggle: (on: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      onClick={() => onToggle(!on)}
      style={{
        flex: "none",
        position: "relative",
        width: 34,
        height: 20,
        padding: 0,
        border: 0,
        borderRadius: 999,
        cursor: disabled ? "default" : "pointer",
        opacity: disabled ? 0.5 : 1,
        background: on ? "var(--color-accent)" : "var(--color-neutral-300)",
        transition: "background .15s",
      }}
    >
      <i
        style={{
          position: "absolute",
          top: 2,
          left: on ? 16 : 2,
          width: 16,
          height: 16,
          borderRadius: "50%",
          background: "#fff",
          boxShadow: "0 1px 2px rgba(0,0,0,.25)",
          transition: "left .15s",
        }}
      />
    </button>
  );
}
