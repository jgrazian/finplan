import type { CSSProperties, ReactNode } from "react";
import { Field } from "./Field";
import { cx } from "./cx";

/**
 * A field that shows whether it is holding an edit.
 *
 * The accent marks the input and its label together, so a change made in a part
 * of the drawer that has since scrolled away is still visibly one of the "n
 * unsaved" the footer counts. Both the account drawer and the event one keep
 * score this way.
 */
export function DirtyField({
  label,
  changed,
  children,
  style,
}: {
  label: ReactNode;
  changed: boolean;
  children: ReactNode;
  style?: CSSProperties;
}) {
  return (
    <Field
      className={cx(changed && "field-dirty")}
      style={style}
      label={
        changed ? (
          <span style={{ display: "flex", alignItems: "center", gap: 5 }}>
            {label}
            <i
              style={{ width: 5, height: 5, background: "var(--color-accent)", display: "block" }}
              aria-hidden
            />
          </span>
        ) : (
          label
        )
      }
    >
      {children}
    </Field>
  );
}
