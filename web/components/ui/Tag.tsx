import type { ReactNode } from "react";
import { cx } from "./cx";

export type TagTone = "accent" | "accent-2" | "neutral" | "outline";

export function Tag({
  tone = "neutral",
  children,
  className,
}: {
  tone?: TagTone;
  children: ReactNode;
  className?: string;
}) {
  return <span className={cx("tag", `tag-${tone}`, className)}>{children}</span>;
}
