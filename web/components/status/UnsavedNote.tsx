import type { ReactNode } from "react";

/**
 * Field state, under the field it belongs to.
 *
 * Never a bar: the status bar carries server state, and a save the server
 * refused is about this input. The value stays where it was typed, and the
 * note says so — losing it silently is the failure people actually fear.
 */
export function UnsavedNote({ children }: { children: ReactNode }) {
  return (
    <div className="field-note">
      <svg
        width="12"
        height="12"
        viewBox="0 0 16 16"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
        aria-hidden="true"
      >
        <circle cx="8" cy="8" r="6.25" />
        <line x1="4.1" y1="11.9" x2="11.9" y2="4.1" />
      </svg>
      <span>{children}</span>
    </div>
  );
}
