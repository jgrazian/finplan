/**
 * What a running model loop — a chat turn, or a review's AI pass — is doing,
 * from the steps the server reports (`AiStep`) to the lines and status shown.
 */
import type { AiStep } from "../api/suggestions.ts";

/** One line of a running model loop's progress. */
export interface ActivityLine {
  key: string;
  /** "Simulating the change", or the model's own words for a narration. */
  text: string;
  /**
   * `working` until it finishes; `failed` for a tool that handed the model
   * an error (it often recovers); `said` for the model's narration;
   * `notice` for a retry.
   */
  tone: "working" | "done" | "failed" | "said" | "notice";
}

/**
 * What each tool call reads as while the model waits on it. A tool this list
 * does not know reads as its name.
 */
const TOOL_LABELS: Record<string, string> = {
  preview_changes: "Simulating the change",
  preview_paths: "Comparing options",
  validate_changes: "Checking the change",
  preflight: "Checking the plan",
  inspect_path: "Looking at one simulated path",
  failure_profile: "Studying where the plan falls short",
  reference_facts: "Looking up tax and benefit rules",
  finance_calc: "Running the numbers",
  estimate_social_security: "Estimating Social Security",
  estimate_taxes: "Estimating taxes",
  goal_seek: "Searching for the amount that reaches the goal",
  submit_suggestion: "Writing a suggestion",
};

/** `rate_limit` (`AiRetryReason`) → what the retry line says. */
const RETRY_LABELS: Record<string, string> = {
  rate_limit: "The model is busy, trying again",
  timeout: "The model stopped responding, trying again",
  network: "The connection dropped, trying again",
};

export function toolLabel(name: string): string {
  return TOOL_LABELS[name] ?? `Using ${name.replaceAll("_", " ")}`;
}

/**
 * A turn's steps as lines. A finished model request is not a line: what it
 * produced (narration, tool calls) already is, so only the request in flight
 * shows, as "Thinking".
 */
export function activityLines(steps: AiStep[]): ActivityLine[] {
  const lines: ActivityLine[] = [];
  steps.forEach((step, i) => {
    const key = `${i}`;
    switch (step.kind) {
      case "thinking":
        if (!step.done) lines.push({ key, text: "Thinking", tone: "working" });
        break;
      case "tool":
        lines.push({
          key,
          text: toolLabel(step.name),
          tone: !step.done ? "working" : step.failed ? "failed" : "done",
        });
        break;
      case "narration":
        lines.push({ key, text: step.text, tone: "said" });
        break;
      case "retry":
        lines.push({
          key,
          text: RETRY_LABELS[step.reason] ?? "The provider had a problem, trying again",
          tone: "notice",
        });
        break;
    }
  });
  return lines;
}

/** The status while a turn runs: the step in progress, or "Thinking…". */
export function runningStatus(lines: ActivityLine[]): string {
  const now = [...lines].reverse().find((line) => line.tone === "working");
  return `${now?.text ?? "Thinking"}…`;
}

/**
 * The review header while the AI pass runs: the last tool it called, in
 * progress or not — the most specific thing to say about where it is.
 */
export function reviewActivity(steps: AiStep[]): string {
  const tool = [...steps].reverse().find((step) => step.kind === "tool");
  if (tool?.kind !== "tool") return "AI review in progress…";
  return `AI review: ${toolLabel(tool.name)}${tool.done ? "" : "…"}`;
}
