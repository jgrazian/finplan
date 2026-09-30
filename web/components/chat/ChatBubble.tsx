import type { ReactNode } from "react";

/**
 * One message in a conversation with FinPlan's model: the user's on the
 * right, tinted; FinPlan's on the left. Shared by Describe & upload and plan
 * chat on the Review tab (the `ns-msg` styles in new-scenario.css).
 */
export function ChatBubble({
  from,
  label,
  children,
  as: Tag = "div",
  ariaLabel,
}: {
  from: "you" | "finplan";
  /** Defaults to "You" or "FinPlan". */
  label?: string;
  children: ReactNode;
  as?: "div" | "li" | "section";
  ariaLabel?: string;
}) {
  return (
    <Tag className={from === "you" ? "ns-msg mine" : "ns-msg"} aria-label={ariaLabel}>
      <span className="ns-lbl">{label ?? (from === "you" ? "You" : "FinPlan")}</span>
      {children}
    </Tag>
  );
}

/** A message's own words: verbatim, line breaks kept, never read as markup. */
export function ChatText({ children }: { children: string }) {
  return <span style={{ textWrap: "pretty", whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{children}</span>;
}
