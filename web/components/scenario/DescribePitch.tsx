"use client";

import type { ReactNode } from "react";
import { useGuest } from "@/components/auth/GuestContext";
import { Button } from "@/components/ui";
import { SetupBar } from "./SetupBar";

/** The mark AI features wear in the setup switch and here. */
export const AI_MARK = "✦";

/**
 * Describe & upload, for someone without an account on a server that has AI:
 * what it would do, shown rather than locked, with the way to an account.
 * The example is illustrative; nothing is sent from this page.
 */
export function DescribePitch({
  modeSwitch,
  onGuided,
}: {
  modeSwitch: ReactNode;
  /** Back to the questions, which need no account. */
  onGuided: () => void;
}) {
  const { openSignUp, openSignIn } = useGuest();
  return (
    <div className="ns-page">
      <SetupBar title="New scenario" modeSwitch={modeSwitch} />
      <section className="ai-pitch" aria-labelledby="ai-pitch-title">
        <div className="ai-pitch-copy">
          <span className="ai-pitch-mark">{AI_MARK} AI-assisted setup</span>
          <h2 id="ai-pitch-title">Describe your situation. AI drafts the plan.</h2>
          <p>
            Write a few sentences about your savings, income, spending and when you would like to
            retire, or upload bank and retirement statements. FinPlan&rsquo;s AI turns them into
            accounts, income and spending, and shows where it found each number so you can check
            it.
          </p>
          <ol className="ai-pitch-steps">
            <li>Describe your plan or upload statements</li>
            <li>Review the draft, every figure beside its source</li>
            <li>Create the plan and see how long your savings last</li>
          </ol>
          <p className="ai-pitch-quiet">
            AI setup needs a free account, since the draft is written on FinPlan&rsquo;s servers.
            Guided setup works without one and keeps your plan in this browser.
          </p>
          <div className="ai-pitch-actions">
            <Button variant="primary" onClick={openSignUp}>
              Create a free account
            </Button>
            <Button onClick={openSignIn}>Sign in</Button>
            <Button variant="ghost" onClick={onGuided}>
              Use guided setup instead
            </Button>
          </div>
        </div>

        <figure className="ai-pitch-example" aria-label="An example of AI-assisted setup">
          <figcaption>Example</figcaption>
          <blockquote>
            &ldquo;I&rsquo;m 42 with about $180k in my 401(k) and $30k in savings. I make $140k
            and we spend around $6k a month. I&rsquo;d like to stop working at 60.&rdquo;
          </blockquote>
          <div className="ai-pitch-arrow" aria-hidden>
            ↓
          </div>
          <dl>
            <div>
              <dt>Savings</dt>
              <dd>$30,000</dd>
            </div>
            <div>
              <dt>401(k)</dt>
              <dd>$180,000</dd>
            </div>
            <div>
              <dt>Salary until 60</dt>
              <dd>$140,000 / yr</dd>
            </div>
            <div>
              <dt>Spending</dt>
              <dd>$6,000 / mo</dd>
            </div>
          </dl>
        </figure>
      </section>
    </div>
  );
}
