import type { CSSProperties, ReactNode } from "react";

/** A keycap, for the keyboard parity carried over from the TUI. */
export function Kbd({ children, style }: { children: ReactNode; style?: CSSProperties }) {
  return (
    <span className="kb" style={style}>
      {children}
    </span>
  );
}
