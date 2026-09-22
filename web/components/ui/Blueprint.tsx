import type { CSSProperties, ReactNode, Ref } from "react";
import { cx } from "./cx";

/**
 * The system's wireframe frame: a hairline box with registration marks at the
 * corners. Wraps anything that should read as a drawn object.
 */
export function Blueprint({
  children,
  className,
  style,
  corners = true,
  ref,
}: {
  children?: ReactNode;
  className?: string;
  style?: CSSProperties;
  /** Omit the registration marks when frames sit flush against each other. */
  corners?: boolean;
  /** For a frame in a draggable list: the box a drop point is measured against. */
  ref?: Ref<HTMLDivElement>;
}) {
  return (
    <div ref={ref} className={cx("blueprint", className)} style={style}>
      {corners && (
        <>
          <i className="corner tl" />
          <i className="corner tr" />
          <i className="corner bl" />
          <i className="corner br" />
        </>
      )}
      {children}
    </div>
  );
}
