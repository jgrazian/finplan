"use client";

import { useMemo } from "react";
import {
  ChartCanvas,
  DEFAULT_GEOMETRY,
  FanSeries,
  Legend,
  domainFor,
  makeScale,
  useChartGeometry,
} from "@/components/charts";
import { PHONE_GEOMETRY } from "@/components/results/NetWorthChart";
import { AI_MARK } from "@/components/scenario/DescribePitch";
import { Button } from "@/components/ui";
import { fmtAxis } from "@/lib/format";
import { useIsMobile } from "@/lib/hooks/useIsMobile";
import { PRIVACY_PROMISE } from "@/lib/local/durability";
import type { NetWorthBands } from "@/lib/types";

/** The illustration's story, in today's dollars: the caption says the same. */
const FIRST_AGE = 40;
const RETIRE_AGE = 65;
const LAST_AGE = 90;
const OPENING = 150_000;
const SAVED_PER_YEAR = 20_000;
const SPENT_PER_YEAR = 55_000;

/**
 * One path at a constant real return: saving until retirement, then spending,
 * never below zero. Each band edge is one of these at a lower or higher return,
 * which is close enough to a run's shape to show what Results draws. It is a
 * drawing, not a simulation, and the caption says so.
 */
function path(rate: number): number[] {
  const values: number[] = [];
  let wealth = OPENING;
  for (let age = FIRST_AGE; age <= LAST_AGE; age++) {
    values.push(wealth);
    wealth = Math.max(0, wealth * (1 + rate) + (age < RETIRE_AGE ? SAVED_PER_YEAR : -SPENT_PER_YEAR));
  }
  return values;
}

const ages = Array.from({ length: LAST_AGE - FIRST_AGE + 1 }, (_, i) => FIRST_AGE + i);
const SAMPLE: NetWorthBands = {
  years: ages,
  ages,
  outer: [10, 90],
  low: path(0.0144),
  lowerQuartile: path(0.0265),
  p50: path(0.04),
  upperQuartile: path(0.0535),
  high: path(0.0656),
};
// Scaled to the inner band, as Results scales its fan.
const DOMAIN = domainFor("linear", [SAMPLE.lowerQuartile, SAMPLE.p50, SAMPLE.upperQuartile]);
const noHover = () => {};

/**
 * What a first visit sees before any plan exists: what FinPlan tells you, what
 * it will ask for, and a picture of the answer, with one button to begin.
 */
export function StartCard({
  onCreate,
  onExample,
  exampleBusy,
  exampleError,
  onImport,
  local,
  account,
  ai,
}: {
  onCreate: () => void;
  /** Makes the example household's plan and opens it running. */
  onExample: () => void;
  exampleBusy?: boolean;
  exampleError?: string;
  /** Local mode only: a backup file can be imported instead. */
  onImport: () => void;
  local: boolean;
  /** A guest's way to an account; shown here on a phone, whose header has no room. */
  account?: { onSignUp: () => void; onSignIn: () => void };
  /** The server has AI: offer AI-assisted setup as what an account adds. */
  ai?: boolean;
}) {
  const [measure, geo] = useChartGeometry(useIsMobile() ? PHONE_GEOMETRY : DEFAULT_GEOMETRY);
  const scale = useMemo(() => makeScale(ages.length, DOMAIN, { geo }), [geo]);
  return (
    <section className="start-card" aria-labelledby="start-title">
      <div className="start-copy">
        <h2 id="start-title">See how long your savings will last</h2>
        <p>
          Enter your accounts, income and spending. FinPlan runs your plan through thousands of
          possible markets and shows the range of outcomes, including how often the money runs
          short.
        </p>
        <p className="start-quiet">
          Setup takes about five minutes, and rough numbers are fine: your balances, salary,
          monthly spending and when you want to retire.
        </p>
        {local && <p className="start-quiet">{PRIVACY_PROMISE}</p>}
        <div className="start-actions">
          <Button variant="primary" onClick={onCreate}>
            Create a scenario
          </Button>
          <Button onClick={onExample} disabled={exampleBusy}>
            {exampleBusy ? "Opening the example…" : "See an example plan"}
          </Button>
        </div>
        {exampleError && (
          <p role="alert" className="start-quiet">
            {exampleError}
          </p>
        )}
        <p className="start-quiet start-more">
          The example is an invented household you can edit freely.
          {local && (
            <>
              {" "}
              <button type="button" className="linkbtn" onClick={onImport}>
                Import a backup file
              </button>
            </>
          )}
        </p>
        {account && ai && (
          <div className="start-ai">
            <span className="start-ai-mark">{AI_MARK} AI-assisted setup</span>
            <p>
              Rather describe it? With a free account, write a few sentences or upload statements,
              and AI drafts the plan for you to review.
            </p>
            <div className="start-ai-actions">
              <Button onClick={account.onSignUp}>Create a free account</Button>
              <button type="button" className="linkbtn" onClick={account.onSignIn}>
                Sign in
              </button>
            </div>
          </div>
        )}
        {account && !ai && (
          <p className="start-quiet start-account">
            <button type="button" className="linkbtn" onClick={account.onSignIn}>
              Sign in
            </button>{" "}
            or{" "}
            <button type="button" className="linkbtn" onClick={account.onSignUp}>
              create a free account
            </button>{" "}
            to keep plans in the cloud.
          </p>
        )}
      </div>

      <figure className="start-sample" aria-label="An illustration of a plan's results">
        <figcaption>
          <strong>Net worth, by age</strong>
          <span>Illustration, not your numbers</span>
        </figcaption>
        <ChartCanvas measureRef={measure} scale={scale} years={ages} format={fmtAxis} hoverIndex={null} onHoverChange={noHover}>
          <FanSeries bands={SAMPLE} scale={scale} />
        </ChartCanvas>
        <Legend
          items={[
            { label: "Median outcome", color: "var(--color-envelope-p50)" },
            { label: "Middle half of outcomes", color: "var(--color-envelope-inner)" },
            { label: "8 in 10 outcomes", color: "var(--color-envelope-outer)" },
          ]}
        />
        <p className="start-quiet">
          Someone aged {FIRST_AGE} with {fmtAxis(OPENING)} saved, adding {fmtAxis(SAVED_PER_YEAR)} a
          year, then spending {fmtAxis(SPENT_PER_YEAR)} a year from {RETIRE_AGE}, in today&rsquo;s
          dollars. In weaker markets the savings run out in their 80s; in the middle they last
          past {LAST_AGE}.
        </p>
      </figure>
    </section>
  );
}
