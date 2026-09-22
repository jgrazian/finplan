import type { ReactNode } from "react";

/**
 * The framed application window. Every screen renders inside one of these,
 * so the blueprint edge and its registration marks are drawn once.
 */
export function AppShell({ children }: { children: ReactNode }) {
  return (
    <div className="app blueprint" style={{ width: "100%", maxWidth: 1440, margin: "0 auto" }}>
      <i className="corner tl" />
      <i className="corner tr" />
      <i className="corner bl" />
      <i className="corner br" />
      {children}
    </div>
  );
}

/**
 * The two-column split both screens use: a main pane with a hairline on its
 * right edge and a fixed-width rail beside it.
 */
export function SplitPane({
  main,
  rail,
  railWidth = 296,
}: {
  main: ReactNode;
  rail: ReactNode;
  railWidth?: number;
}) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: `1fr ${railWidth}px`,
        alignItems: "stretch",
      }}
    >
      <div style={{ borderRight: "1px solid var(--color-divider)", minWidth: 0 }}>{main}</div>
      <aside>{rail}</aside>
    </div>
  );
}
