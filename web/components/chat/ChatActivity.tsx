import type { ActivityLine } from "@/lib/view/chat";

const MARKS: Record<ActivityLine["tone"], string> = {
  working: "",
  done: "✓",
  failed: "✕",
  said: "",
  notice: "↻",
};

/**
 * What a running chat turn has done so far, in place of a bare "Thinking…":
 * the tools the model is calling, what it says on the way, and retries. The
 * line still in progress pulses; the rest are a quiet record above it.
 *
 * A live region, so a screen reader hears each step as it lands.
 */
export function ChatActivity({ lines }: { lines: ActivityLine[] }) {
  return (
    <ol className="chat-activity" role="status" aria-live="polite" aria-label="What the model is doing">
      {lines.map((line) => (
        <li key={line.key} data-tone={line.tone}>
          <span className="chat-activity-mark" aria-hidden>
            {MARKS[line.tone]}
          </span>
          <span className="chat-activity-text">
            {line.tone === "working" ? `${line.text}…` : line.text}
            {line.tone === "failed" && <span className="sr-only"> (failed)</span>}
          </span>
        </li>
      ))}
    </ol>
  );
}
