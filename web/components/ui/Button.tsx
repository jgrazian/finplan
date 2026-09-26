"use client";

import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cx } from "./cx";
import { Kbd } from "./Kbd";

type Variant = "primary" | "secondary" | "ghost" | "add";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  /** Renders a keycap after the label, e.g. `r` for Run. */
  shortcut?: string;
  block?: boolean;
  icon?: boolean;
  children?: ReactNode;
}

export function Button({
  variant = "secondary",
  shortcut,
  block,
  icon,
  className,
  children,
  type = "button",
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      className={cx(
        "btn",
        `btn-${variant}`,
        block && "btn-block",
        icon && "btn-icon",
        className,
      )}
      {...rest}
    >
      {children}
      {shortcut && variant !== "add" && (
        <Kbd style={variant === "primary" ? { borderColor: "currentColor" } : undefined}>
          {shortcut}
        </Kbd>
      )}
    </button>
  );
}
