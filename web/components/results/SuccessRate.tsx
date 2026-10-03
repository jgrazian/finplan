import type { CSSProperties, ReactNode } from "react";
import { IterationUpsell, useGuest } from "@/components/auth/GuestContext";
import { StatLabel, Tooltip } from "@/components/ui";
import { fmtInt } from "@/lib/format";
import { useCountUp } from "@/lib/hooks/useCountUp";
import { intervalLabel, successIntervalPoints } from "@/lib/view/guest";
import {
  SUCCESS_BANDS,
  SUCCESS_SCALE_FLOOR,
  successBand,
  successScalePosition,
} from "@/lib/view/outcome";

/** Funding across the path and terminal wealth are different measurements. */
export function SuccessRate({
  fundingSuccessRate,
  iterations,
  action,
}: {
  /** Absent on runs that predate checkpoint funding checks. */
  fundingSuccessRate?: number;
  iterations: number;
  /**
   * Set beside the label, right-aligned. The phone layout puts the iteration
   * menu here, since the rail it lives in on desktop is below the fold.
   */
  action?: ReactNode;
}) {
  const measured = fundingSuccessRate != null;
  const fraction = fundingSuccessRate ?? 0;
  const pct = fraction * 100;
  const passed = Math.round(iterations * fraction);
  const other = iterations - passed;
  const label = "Cash funding check";
  // The figure, its verdict and the scale's marker all follow the rolling
  // value, so a run landing never shows 91.5 beside a "Workable" verdict.
  const shownPct = useCountUp(pct);
  const shown = shownPct / 100;
  const band = successBand(shown);
  // A guest's runs are small enough that the rate moves by several points
  // from one run to the next, so the headline carries how far.
  const { restricted } = useGuest();
  const noise = restricted && measured ? successIntervalPoints(fraction, iterations) : undefined;

  return (
    <section aria-label="Simulation outcome definitions" style={{ marginBottom: 20 }}>
      <div
        className="success-rate"
        style={{ display: "flex", alignItems: "flex-end", gap: 32, flexWrap: "wrap" }}
      >
        <div className="success-figure">
          {action ? (
            <div className="success-head">
              <StatLabel>{label}</StatLabel>
              {action}
            </div>
          ) : (
            <StatLabel>{label}</StatLabel>
          )}
          <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
            <span
              className="success-pct"
              style={{
                fontFamily: "var(--font-display)",
                fontWeight: "var(--font-display-weight)",
                fontSize: 68,
                lineHeight: 1,
                letterSpacing: "-0.035em",
              }}
            >
              {measured ? shownPct.toFixed(1) : "—"}
            </span>
            <span
              className="success-pct-unit"
              style={{ fontFamily: "var(--font-display)", fontSize: 28, color: "var(--color-accent-700)" }}
            >
              %
            </span>
            {noise != null && noise > 0 && (
              <span
                title={`95% interval from ${fmtInt(iterations)} iterations: the true rate is likely within ${noise.toFixed(1)} percentage points of this figure.`}
                style={{ fontFamily: "var(--font-display)", fontSize: 28 }}
              >
                ± {intervalLabel(noise)}
              </span>
            )}
          </div>
          <div style={{ fontSize: 12 }}>
            {fmtInt(iterations)} Monte Carlo iterations
            {measured && (
              <>
                {" · "}
                <span className={`success-verdict is-${band.key}`}>{band.label}</span>
              </>
            )}
            {restricted && (
              <>
                {" · "}
                <IterationUpsell />
              </>
            )}
          </div>
        </div>
        {measured && (
          <div className="success-bar" style={{ flex: 1, minWidth: 200, paddingBottom: 6 }}>
            <SuccessScale rate={shown} settled={fraction} label={label} />
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                gap: 12,
                fontSize: 12,
                marginTop: 8,
              }}
            >
              {/* The phone keeps the counts and the first word; the rest of
                  each phrase is the desktop's room to explain itself. */}
              <span>
                {fmtInt(passed)} passed<span className="success-long"> the funding check</span>
              </span>
              <span>
                {fmtInt(other)} <span className="success-long">had a </span>shortfall
                <span className="success-long"> or event warning</span>
                <span className="success-short">…</span>
              </span>
            </div>
          </div>
        )}
      </div>
      {!measured && (
        <p style={{ fontSize: 13, margin: "10px 0 0", maxWidth: 850 }}>
          Not measured — rerun to measure cash funding.
        </p>
      )}
    </section>
  );
}

/**
 * The rate read against a rule-of-thumb scale rather than as a bare fill: the
 * three bands are drawn from 50% up, so the stretch where plans actually
 * differ gets the width, and the band the rate lands in is named in type as
 * well as tint — the colour never carries the verdict alone.
 */
function SuccessScale({
  rate,
  settled,
  label,
}: {
  /** The value drawn — rolling while a new run lands. */
  rate: number;
  /** The run's actual rate, which is what a screen reader is told. */
  settled: number;
  label: string;
}) {
  const here = successBand(rate);
  const said = successBand(settled);
  const at = (from: number) => successScalePosition(Math.max(from, SUCCESS_SCALE_FLOOR)) * 100;
  const pct = (settled * 100).toFixed(1);

  return (
    <div className="success-scale">
      <div
        className="success-track"
        role="img"
        aria-label={`${label}: ${pct}%, ${said.label.toLowerCase()} — ${said.meaning}`}
      >
        {SUCCESS_BANDS.map((band, i) => {
          const left = at(band.from);
          const right = i + 1 < SUCCESS_BANDS.length ? at(SUCCESS_BANDS[i + 1].from) : 100;
          return (
            <span
              key={band.key}
              className={`success-zone is-${band.key}${band.key === here.key ? " is-here" : ""}`}
              style={{ left: `${left}%`, width: `${right - left}%` }}
            />
          );
        })}
        <span
          className="success-marker"
          style={{ "--at": `${successScalePosition(rate) * 100}%` } as CSSProperties}
        />
      </div>
      <div className="success-zone-labels" aria-hidden="true">
        {SUCCESS_BANDS.map((band, i) => {
          const left = at(band.from);
          const right = i + 1 < SUCCESS_BANDS.length ? at(SUCCESS_BANDS[i + 1].from) : 100;
          return (
            <span
              key={band.key}
              className={band.key === here.key ? "is-here" : undefined}
              style={{ left: `${left}%`, width: `${right - left}%` }}
            >
              {band.label}
              <small>{band.from <= SUCCESS_SCALE_FLOOR ? "<75%" : `${Math.round(band.from * 100)}%+`}</small>
            </span>
          );
        })}
      </div>
      <div className="success-meaning">
        <Tooltip
          content={
            "Rules of thumb, not advice. Planners commonly treat under 75% as fragile, " +
            "75–90% as workable and 90%+ as robust. A rate near 100% can also mean the plan " +
            "is spending less than it safely could."
          }
        >
          {here.meaning}
        </Tooltip>
      </div>
    </div>
  );
}
