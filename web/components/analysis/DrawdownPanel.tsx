"use client";

import { useEffect, useMemo, useState } from "react";
import { EmptyState } from "@/components/screens/EmptyState";
import { Button, Dialog, SegmentedControl } from "@/components/ui";
import type {
  DrawdownBody,
  DrawdownComparison,
  Run,
  Scenario,
} from "@/lib/api/types";
import { ApiError } from "@/lib/api/http";
import { useAsync } from "@/lib/hooks/useAsync";
import { useIsMobile } from "@/lib/hooks/useIsMobile";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { useNav, usePlanApi } from "@/lib/nav";
import { conversionWrites } from "@/lib/view/conversion";
import {
  DEFAULT_CEILING,
  ceilingOf,
  choiceKey,
  choiceLabel,
  choiceOfKey,
  conversionKey,
  conversionLabel,
  drawdownCsv,
  drawdownView,
  isCurrentChoice,
  policyFor,
  requestChoices,
  retirementHint,
  yearPanel,
  type ConversionSetting,
  type DrawdownBasis,
  type DrawdownUnit,
} from "@/lib/view/drawdown";
import { ConversionToggle } from "./ConversionToggle";
import { DrawdownChart, DrawdownLegend } from "./DrawdownChart";
import { DrawdownSide } from "./DrawdownSide";
import { StrategyChips } from "./StrategyChips";
import { StrategyComparison } from "./StrategyComparison";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";

const UNITS = [
  { value: "usd", label: "Dollars" },
  { value: "share", label: "Share" },
] as const;
const BASES = [
  { value: "nominal", label: "Nominal" },
  { value: "real", label: "Today’s $" },
] as const;

function when(run: Run): string {
  const at = new Date(run.finished_at ?? run.created_at);
  return at.toLocaleString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

/**
 * Analysis > Drawdown (spec 20): which accounts fund each year of retirement
 * spending, on the median market path of the plan's latest successful run,
 * under each withdrawal strategy.
 */
export function DrawdownPanel({
  scenario,
  onPlanChanged,
}: {
  scenario: Scenario;
  /** The plan was written to: reload whatever the app holds of it. */
  onPlanChanged: () => void;
}) {
  const api = usePlanApi();
  const nav = useNav();
  const narrow = useIsMobile();

  const runs = useAsync(() => api.runs.list(scenario.id), [api, scenario.id]);
  const run = runs.data?.find((r) => r.status === "succeeded");

  // The bracket ceiling lives in the strategy key (`bracket-22`), so a link
  // reopens on it; changing it re-asks for the whole list.
  const picked = choiceOfKey(nav.selection);
  const [ceiling, setCeiling] = useState(() => (picked && ceilingOf(picked)) ?? DEFAULT_CEILING);
  const fundingKey = JSON.stringify(scenario.funding);
  // The Roth conversion toggle applies to every strategy at once.
  const [conversion, setConversion] = useState<ConversionSetting>();
  const conversionAt = conversionKey(conversion);

  const drawdown = useAsync<DrawdownBody | undefined>(async () => {
    if (!run) return undefined;
    return api.runs.drawdown(run.id, {
      strategies:
        ceiling === DEFAULT_CEILING && conversion == null ? undefined : requestChoices(ceiling, conversion),
    });
    // conversion is carried by conversionAt.
  }, [api, run?.id, ceiling, fundingKey, conversionAt]);
  const body = drawdown.data;

  // The comparison is slower than the chart, so it follows it and can be
  // abandoned when the question changes.
  const compareKey = body && run ? `${run.id}:${ceiling}:${fundingKey}:${conversionAt}` : undefined;
  const [compared, setCompared] = useState<{
    key: string;
    data?: DrawdownComparison;
    error?: string;
  }>();
  useEffect(() => {
    if (!compareKey || !run) return;
    const abort = new AbortController();
    api.runs
      .drawdownCompare(
        run.id,
        { request: { strategies: requestChoices(ceiling, conversion) } },
        abort.signal,
      )
      .then((data) => setCompared({ key: compareKey, data }))
      .catch((e: unknown) => {
        if (abort.signal.aborted) return;
        setCompared({ key: compareKey, error: e instanceof Error ? e.message : String(e) });
      });
    return () => abort.abort();
    // run, ceiling and conversion are carried by compareKey.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, compareKey]);
  const comparison = compared?.key === compareKey ? compared : undefined;

  const [unit, setUnit] = useState<DrawdownUnit>("usd");
  const [basis, setBasis] = useState<DrawdownBasis>("nominal");
  const [hover, setHover] = useState<number | null>(null);
  const [applying, setApplying] = useState(false);
  const [notice, setNotice] = useState<string>();
  const submit = useSubmit();

  const selected = useMemo(() => {
    if (!body) return -1;
    const wanted = picked ? choiceKey(picked) : undefined;
    const byKey = body.choices.findIndex((c) => choiceKey(c.choice) === wanted);
    if (byKey >= 0) return byKey;
    const current = body.choices.findIndex((c) => isCurrentChoice(c.choice, body.plan_funding));
    return current >= 0 ? current : 0;
  }, [body, picked]);
  const choice = body && selected >= 0 ? body.choices[selected] : undefined;
  const view = useMemo(
    () => (body && choice ? drawdownView(body, choice, unit, basis) : undefined),
    [body, choice, unit, basis],
  );

  const pinnedIndex = view && nav.year != null ? view.years.indexOf(nav.year) : -1;
  const pinned = pinnedIndex >= 0 ? pinnedIndex : null;
  const shownIndex = hover ?? pinned ?? 0;
  const panel = body && choice && view ? yearPanel(body, choice, view, shownIndex) : undefined;

  // ── states ──
  if (runs.loading && !runs.data) {
    return <EmptyState title="Loading…" detail="Finding the plan’s latest run." />;
  }
  if (runs.error) return <EmptyState title="Cannot read this plan’s runs" detail={runs.error.message} />;
  if (!run) {
    return (
      <EmptyState
        title="Run the plan first"
        detail="Drawdown reads a finished run. Run the plan from the Results tab, then come back to compare how its accounts fund retirement."
      />
    );
  }
  if (drawdown.error) {
    const stale = drawdown.error instanceof ApiError && drawdown.error.status === 409;
    return (
      <EmptyState
        title={stale ? "Run the plan again to see drawdown." : "Drawdown could not be read"}
        detail={
          stale
            ? "This run predates drawdown support, so it did not keep the market path it needs."
            : drawdown.error.message
        }
      />
    );
  }
  if (!body || !choice || !view) {
    return <EmptyState title="Loading…" detail="Replaying the median market path under each strategy." />;
  }

  const select = (key: string) => {
    const next = choiceOfKey(key);
    if (next && ceilingOf(next) != null) setCeiling(ceilingOf(next)!);
    nav.setSelection(key);
  };
  const pin = (index: number | null) =>
    nav.setYear(index == null ? undefined : view.years[index]);
  const key = choiceKey(choice.choice);
  const hint = retirementHint(body.retirement.source);
  const funding = policyFor(choice.choice, scenario.funding);
  const canApply = funding != null || conversion != null;

  // Funding first, then the conversion toggle, against the plan's events as
  // they are now: the view ran on the run's snapshot.
  const apply = () => {
    if (!canApply) return;
    submit.run(
      async () => {
        if (funding) await api.scenarios.setFunding(scenario.id, { funding, align_sweeps: true });
        if (conversion != null) {
          const events = await api.events.list(scenario.id);
          for (const write of conversionWrites(events, conversion, body.conversions.overlay)) {
            if (write.kind === "create") await api.events.create(scenario.id, write.body);
            else await api.events.replace(scenario.id, write.id, write.body);
          }
        }
      },
      () => {
        setApplying(false);
        const done = [
          funding ? `sells investments using ${choiceLabel(choice.choice)}` : undefined,
          conversion == null
            ? undefined
            : conversion.kind === "Off"
              ? "makes no Roth conversions"
              : `converts to a Roth up to the ${Math.round(conversion.ceiling_rate * 100)}% bracket`,
        ].filter(Boolean);
        setNotice(`The plan now ${done.join(" and ")}. Run the plan again to see its effect.`);
        onPlanChanged();
      },
    );
  };
  const overlay = body.conversions.overlay;
  const accountName = (id: number | null | undefined) =>
    body.accounts.find((a) => a.id === id)?.name ?? "the account";

  const download = () => {
    const blob = new Blob([drawdownCsv(body, choice, basis)], { type: "text/csv" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `drawdown-${key}-${basis}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14, padding: "14px 20px 28px", minWidth: 0 }}>
      <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: "6px 12px" }}>
        <span style={{ fontSize: 13, color: MUTED, flex: "1 1 280px" }}>
          Last run {when(run)} · median market path · retire {body.retirement.year}
          {body.retirement.age != null ? ` at ${body.retirement.age}` : ""}
          {hint ? ` (${hint})` : ""}
        </span>
        <Button variant="ghost" onClick={download}>
          Export CSV
        </Button>
        <Button
          variant="primary"
          disabled={!canApply}
          title={canApply ? undefined : "Pick a strategy or a conversion setting to apply"}
          onClick={() => {
            submit.fail("");
            setApplying(true);
          }}
        >
          Apply to plan
        </Button>
      </div>

      {notice && (
        <p role="status" style={{ margin: 0, fontSize: 13 }}>
          {notice}{" "}
          <button type="button" onClick={() => setNotice(undefined)} style={{ all: "unset", cursor: "pointer", textDecoration: "underline" }}>
            Dismiss
          </button>
        </p>
      )}

      {choice.overlay && (
        <div
          role="note"
          style={{
            display: "flex",
            flexWrap: "wrap",
            alignItems: "center",
            gap: 10,
            padding: "10px 14px",
            fontSize: 13,
            border: "1px solid var(--color-divider)",
            background: "color-mix(in srgb, var(--color-accent-2) 10%, transparent)",
          }}
        >
          <span style={{ flex: "1 1 300px" }}>
            Your plan doesn&apos;t sell investments to pay for spending. This view assumes it does
            from {body.retirement.year}.
          </span>
          {canApply && (
            <Button onClick={() => { submit.fail(""); setApplying(true); }}>Turn it on</Button>
          )}
        </div>
      )}

      {choice.conversion_overlay && overlay && (
        <div
          role="note"
          style={{
            display: "flex",
            flexWrap: "wrap",
            alignItems: "center",
            gap: 10,
            padding: "10px 14px",
            fontSize: 13,
            border: "1px solid var(--color-divider)",
            background: "color-mix(in srgb, var(--color-accent-2) 10%, transparent)",
          }}
        >
          <span style={{ flex: "1 1 300px" }}>
            Your plan has no Roth conversions. This view adds them: each Dec 30 from{" "}
            {overlay.start_year} until age {overlay.until_age}, {accountName(overlay.from_account_id)} to{" "}
            {accountName(overlay.to_account_id)}, the tax paid{" "}
            {overlay.pay_tax_from_account_id == null
              ? "out of the conversion"
              : `from ${accountName(overlay.pay_tax_from_account_id)}`}
            .
          </span>
          <Button onClick={() => { submit.fail(""); setApplying(true); }}>Add to plan</Button>
        </div>
      )}

      <StrategyChips
        choices={body.choices}
        selected={selected}
        onSelect={select}
        planFunding={body.plan_funding}
        ceiling={ceiling}
        onCeiling={(c) => {
          setCeiling(c);
          nav.setSelection(`bracket-${Math.round(c * 100)}`);
        }}
      />
      <ConversionToggle body={body} value={conversion} onChange={setConversion} />

      <div style={{ display: "flex", flexWrap: "wrap", gap: 20, alignItems: "flex-start" }}>
        <section style={{ flex: "999 1 520px", minWidth: 0, display: "flex", flexDirection: "column", gap: 10 }}>
          <div style={{ display: "flex", flexWrap: "wrap", alignItems: "baseline", gap: "6px 12px" }}>
            <h4 style={{ margin: 0, fontFamily: "var(--font-heading)", fontSize: 21 }}>
              Spending and sources{" "}
              <span style={{ fontSize: 15, color: MUTED }}>by year, median market path</span>
            </h4>
            <div style={{ marginLeft: "auto", display: "flex", gap: 8, flexWrap: "wrap" }}>
              <SegmentedControl ariaLabel="Unit" options={UNITS} value={unit} onChange={setUnit} />
              <SegmentedControl ariaLabel="Basis" options={BASES} value={basis} onChange={setBasis} />
            </div>
          </div>
          <DrawdownLegend view={view} />
          <DrawdownChart
            view={view}
            hover={hover}
            onHover={setHover}
            pinned={pinned}
            onPin={pin}
            narrow={narrow}
          />
        </section>
        <div style={{ flex: "1 1 300px", maxWidth: 360, minWidth: 0 }}>
          <DrawdownSide
            view={view}
            panel={panel}
            pinned={pinned != null}
            onClear={() => pin(null)}
            fixedSweeps={body.fixed_sweeps}
          />
        </div>
      </div>

      <StrategyComparison
        comparison={comparison?.data}
        loading={!comparison}
        error={comparison?.error}
        selectedKey={key}
        onSelect={select}
        basis={basis}
        conversion={conversion == null ? undefined : conversionLabel(conversion)}
      />

      {applying && (
        <Dialog
          title="Apply to plan"
          onClose={() => setApplying(false)}
          onSubmit={apply}
          submitLabel="Apply"
          busy={submit.busy}
          error={submit.error}
          width={480}
        >
          {funding && (
            <p style={{ margin: 0, fontSize: 13 }}>
              This sets the plan&apos;s funding policy to <b>{choiceLabel(choice.choice)}</b>
              {ceilingOf(choice.choice) != null ? ` (up to the ${Math.round(ceiling * 100)}% bracket)` : ""}:
              when cash runs short, it sells investments in that order. Withdrawal rules that follow a
              strategy are aligned to it too.
            </p>
          )}
          {conversion != null && (
            <p style={{ margin: 0, fontSize: 13 }}>
              {conversion.kind === "Off"
                ? "Roth conversions: the plan's conversion events are switched off."
                : body.conversions.events.length > 0 || !overlay
                  ? `Roth conversions: the plan's conversion events fill to the top of the ${Math.round(conversion.ceiling_rate * 100)}% bracket each year.`
                  : `Roth conversions: adds a yearly Dec 30 conversion from ${accountName(overlay.from_account_id)} to ${accountName(overlay.to_account_id)}, up to the top of the ${Math.round(conversion.ceiling_rate * 100)}% bracket, from ${overlay.start_year} until age ${overlay.until_age}.`}
            </p>
          )}
          <p style={{ margin: 0, fontSize: 13, color: MUTED }}>
            The plan needs a new run before its results reflect the change.
          </p>
        </Dialog>
      )}
    </div>
  );
}
