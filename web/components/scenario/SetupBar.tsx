import type { ReactNode } from "react";

/** The strip under the header: what this page is, how to start, and the plan's frame. */
export function SetupBar({
  title,
  modeSwitch,
  children,
  end,
}: {
  title: string;
  modeSwitch: ReactNode;
  children?: ReactNode;
  end?: ReactNode;
}) {
  return (
    <div className="ns-bar">
      <span className="stat-l">{title}</span>
      {modeSwitch}
      {children}
      {end != null && <span className="ns-bar-end">{end}</span>}
    </div>
  );
}
