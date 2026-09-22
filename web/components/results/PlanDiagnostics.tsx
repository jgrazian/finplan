import { Blueprint, Button } from "@/components/ui";
import type { SimulationWarning } from "@/lib/types";
import { fundingDiagnostics } from "./diagnostics";

/** Observations for the loaded representative path; not all-iteration statistics. */
export function PlanDiagnostics({
  warnings,
  pathLabel,
  onReviewPlan,
}: {
  warnings: SimulationWarning[];
  pathLabel: string;
  onReviewPlan?: () => void;
}) {
  const { shortfalls, processing } = fundingDiagnostics(warnings);
  if (shortfalls.length === 0 && processing.length === 0) return null;

  return (
    <Blueprint style={{ padding: "14px 16px", margin: "18px 0" }}>
      <section aria-label="Selected path funding diagnostics">
        <h4 style={{ margin: "0 0 6px" }}>
          {shortfalls.length > 0 ? "Cash ran short in this path" : "Some planned events could not be processed"}
        </h4>
        <p style={{ margin: "0 0 10px", fontSize: 13 }}>
          These observations belong to {pathLabel}. Other simulated paths can fail
          at different times or have different warnings.
        </p>
        {shortfalls.length > 0 && (
          <>
            <p style={{ fontSize: 13 }}>
              A modeled cash account fell below zero after scheduled activity
              settled. Investments or property elsewhere do not automatically
              fund that account. Positive ending net worth can coexist with a
              cash shortfall.
            </p>
            <ul>
              {shortfalls.map((warning) => <li key={warning.id}>{warning.detail}</li>)}
            </ul>
            <p style={{ fontSize: 13 }}>
              Review the cash-flow ledger below and the spending source in your
              plan. Add or adjust an explicit transfer or withdrawal if that
              reflects how you intend to fund it, then rerun.
            </p>
          </>
        )}
        {processing.length > 0 && (
          <>
            <p style={{ fontSize: 13 }}>
              Event-processing warnings also fail the cash-funding check. Review
              these events before interpreting the result.
            </p>
            <ul>
              {processing.map((warning) => <li key={warning.id}>{warning.detail}</li>)}
            </ul>
          </>
        )}
        {onReviewPlan && <Button onClick={onReviewPlan}>Review plan events</Button>}
      </section>
    </Blueprint>
  );
}
