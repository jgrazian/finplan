import type { CSSProperties, ReactNode } from "react";

/**
 * The framed application window. Every screen renders inside one of these,
 * so the blueprint edge and its registration marks are drawn once.
 */
export function AppShell({ children }: { children: ReactNode }) {
  return (
    <div className="app app-shell blueprint">
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
 * right edge and a fixed-width rail beside it. On a phone the rail drops
 * below the main pane.
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
      className="split-pane"
      style={{ "--rail-width": `${railWidth}px` } as CSSProperties}
    >
      <div className="split-pane-main">{main}</div>
      <aside className="split-pane-rail">{rail}</aside>
    </div>
  );
}
