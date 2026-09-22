import type { ReactNode } from "react";

/**
 * The h6 kicker that titles every panel, optionally with an action on the
 * right (an "Add lot" ghost button, a segmented control).
 */
export function SectionHeading({
  children,
  action,
  className,
}: {
  children: ReactNode;
  action?: ReactNode;
  className?: string;
}) {
  if (!action) return <h6 className={className} style={{ margin: 0 }}>{children}</h6>;
  return (
    <div
      className={className}
      style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}
    >
      <h6 style={{ margin: 0 }}>{children}</h6>
      {action}
    </div>
  );
}
