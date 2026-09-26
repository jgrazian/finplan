"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { Button, Dialog, Field, Input } from "@/components/ui";
import { api } from "@/lib/api/client";
import { ApiError } from "@/lib/api/http";
import type {
  AnalysisParameter,
  Scenario,
  WhatIfEntry,
  WhatIfOutcome,
} from "@/lib/api/types";
import { fmtInt } from "@/lib/format";
import { useAnalysis } from "@/lib/hooks/useAnalysis";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { useWhatIfStack } from "@/lib/hooks/useWhatIf";
import { planAxis } from "@/lib/view/axis";
import {
  MAX_RUN_LAYERS,
  addOptions,
  applyLines,
  changedSummary,
  fanView,
  impactText,
  layerImpacts,
  layerView,
  newLayer,
  runKey,
  runnableEntries,
  stepLayer,
  waterfallView,
  whatIfStats,
  type AddOption,
  type WhatIfContext,
} from "@/lib/view/whatIf";
import { OverrideCard } from "./OverrideCard";
import { SuccessWaterfall } from "./SuccessWaterfall";
import { WhatIfFan } from "./WhatIfFan";

/** How long the stack has to sit still before it is re-measured. */
const RUN_SETTLE_MS = 200;

/**
 * Simulations for the whole stack, per pass. Every edit gets the quick pass so
 * the numbers move while the user is still stepping; once it lands the same
 * stack is measured again at the refined size, and that answer replaces it.
 * Both run on the analysis seed, so the refinement tightens rather than jumps.
 *
 * The quick pass is one request answered inline — no job to start, poll and
 * fetch — because its whole value is arriving fast. The refinement is a job.
 */
const QUICK_ITERATIONS = 200;
const REFINED_ITERATIONS = 2_000;

type Pass = "quick" | "refined";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";

/** Retries for a quick pass the server turned away as busy (409). */
const QUICK_BUSY_RETRIES = 3;
const QUICK_BUSY_WAIT_MS = 150;

/** The answer on screen, and the stack it answers. */
interface Shown {
  outcome: WhatIfOutcome;
  /** Entry ids of the layers it ran, in order — what the impacts are keyed by. */
  ranFor: string[];
  key: string;
  pass: Pass;
}

/** A stable key for a new entry. Only this client reads it. */
function entryId(): string {
  return typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

/**
 * What-if (18a): an override stack over a copy of the plan.
 *
 * Each card is a layer that can be switched off and stepped; the enabled ones
 * are measured together, one step per prefix, so the waterfall can say what
 * each one costs or buys on top of the ones above it. The plan is never
 * touched until Apply, and Save as scenario writes the same changes into a
 * copy instead.
 */
export function WhatIfPanel({
  scenario,
  parameters,
  onPlanChanged,
  onScenarioCreated,
}: {
  scenario: Scenario;
  parameters: AnalysisParameter[];
  /** The plan was written to: reload whatever the app holds of it. */
  onPlanChanged: () => void;
  /** Save as scenario made a copy: open it. */
  onScenarioCreated: (created: Scenario) => void;
}) {
  const ctx = useMemo<WhatIfContext>(() => {
    const axis = planAxis(scenario);
    return {
      parameters,
      ages: axis.unit === "age" ? { start: axis.range[0], end: axis.range[1] } : null,
    };
  }, [scenario, parameters]);

  const stack = useWhatIfStack(scenario.id);
  const entries = stack.entries;
  const job = useAnalysis(scenario.id, "what-if");

  const runnable = useMemo(
    () => (entries ? runnableEntries(entries, ctx) : undefined),
    [entries, ctx],
  );
  const key = runnable ? runKey(runnable) : undefined;
  const tooMany = runnable != null && runnable.length > MAX_RUN_LAYERS;

  // ── measuring ────────────────────────────────────────────────────────
  // Which stack each started job was asked about, so an answer is matched to
  // its own question however the jobs overlap.
  const [requests, setRequests] = useState<Record<number, { ranFor: string[]; key: string }>>(
    {},
  );
  const [requested, setRequested] = useState<string>();
  /** The stack whose refinement has been asked for, so it is asked once. */
  const refining = useRef<string>(undefined);
  /** The quick request in flight, aborted when the stack moves on. */
  const quickFlight = useRef<AbortController>(undefined);
  const [quickError, setQuickError] = useState<string>();
  const [shown, setShown] = useState<Shown>();
  /** The refinement job last put on screen, so it is taken once. */
  const [taken, setTaken] = useState<number>();

  // A refinement is only worth showing for the stack still on screen: one for
  // a stack already left would replace a newer quick answer with an older one.
  const refined = job.resultsFor != null ? requests[job.resultsFor] : undefined;
  if (
    job.results &&
    job.resultsFor != null &&
    job.resultsFor !== taken &&
    refined &&
    refined.key === key
  ) {
    setTaken(job.resultsFor);
    setShown({ outcome: job.results, ...refined, pass: "refined" });
  }

  // The job's controls change identity as it polls; the timer below reads
  // them through a ref so a poll does not restart the settle delay.
  const control = useRef({ start: job.start, cancel: job.cancel, active: job.active });
  useEffect(() => {
    control.current = { start: job.start, cancel: job.cancel, active: job.active };
  }, [job.start, job.cancel, job.active]);

  useEffect(() => {
    if (!runnable || key == null || tooMany || key === requested) return;
    const layers = runnable.map((e) => e.layer);
    const ranFor = runnable.map((e) => e.id);
    const timer = setTimeout(() => {
      setRequested(key);
      setQuickError(undefined);
      // A new stack earns its own refinement, even one seen before and left.
      refining.current = undefined;
      // The answers to the previous stack are no longer wanted.
      const { cancel, active } = control.current;
      if (active) void cancel();
      quickFlight.current?.abort();
      const flight = new AbortController();
      quickFlight.current = flight;

      const ask = async (retries: number): Promise<WhatIfOutcome> => {
        try {
          return await api.whatIf.quick(
            scenario.id,
            { layers, iterations: QUICK_ITERATIONS },
            flight.signal,
          );
        } catch (err) {
          // Busy is usually the refinement just cancelled still letting go.
          if (retries > 0 && err instanceof ApiError && err.status === 409) {
            await new Promise((r) => setTimeout(r, QUICK_BUSY_WAIT_MS));
            if (!flight.signal.aborted) return ask(retries - 1);
          }
          throw err;
        }
      };
      ask(QUICK_BUSY_RETRIES).then(
        (outcome) => {
          if (flight.signal.aborted) return;
          setQuickError(undefined);
          setShown({ outcome, ranFor, key, pass: "quick" });
        },
        (err: unknown) => {
          if (flight.signal.aborted) return;
          setQuickError(err instanceof Error ? err.message : String(err));
        },
      );
    }, RUN_SETTLE_MS);
    return () => clearTimeout(timer);
  }, [runnable, key, tooMany, requested, scenario.id]);

  // Leaving the screen withdraws the question, and the server stops on it.
  useEffect(() => () => quickFlight.current?.abort(), []);

  // The quick answer to the stack on screen is in: queue its refinement.
  useEffect(() => {
    if (!shown || shown.pass !== "quick" || shown.key !== key || refining.current === key) return;
    refining.current = key;
    const layers = (runnable ?? []).map((e) => e.layer);
    const { ranFor } = shown;
    void control.current
      .start({ kind: "what-if", layers, iterations: REFINED_ITERATIONS })
      .then((started) => {
        if (started) {
          setRequests((r) => ({ ...r, [started.id]: { ranFor, key } }));
        }
      });
  }, [shown, key, runnable]);

  const outcome = shown?.outcome;
  // A quick answer to this stack counts as current: the refinement only
  // sharpens it, and the screen says so rather than dimming.
  const current = shown != null && shown.key === key;
  const refiningNow =
    current &&
    shown.pass === "quick" &&
    !job.error &&
    Object.values(requests).some((r) => r.key === key);
  const impacts = useMemo(
    () => (shown ? layerImpacts(shown.outcome, shown.ranFor) : undefined),
    [shown],
  );

  // ── editing ──────────────────────────────────────────────────────────
  const edit = (next: WhatIfEntry[]) => stack.setEntries(next);
  const patch = (id: string, change: (e: WhatIfEntry) => WhatIfEntry) =>
    entries && edit(entries.map((e) => (e.id === id ? change(e) : e)));

  const add = (option: AddOption) => {
    if (!entries) return;
    const layer = newLayer(option.choice, ctx, outcome?.plan_retirement_age);
    edit([...entries, { id: entryId(), enabled: true, layer }]);
  };

  // ── writing ──────────────────────────────────────────────────────────
  const [dialog, setDialog] = useState<"apply" | "save">();
  const [copyName, setCopyName] = useState("");
  const submit = useSubmit();
  const lines = entries ? applyLines(entries, ctx) : [];
  const canWrite = runnable != null && runnable.length > 0 && !tooMany;

  const applyNow = () =>
    submit.run(
      () => api.whatIf.apply(scenario.id, { layers: (runnable ?? []).map((e) => e.layer) }),
      () => {
        setDialog(undefined);
        // The server has cleared its copy of the stack; a queued write of the
        // old one must not put it back.
        stack.forget();
        onPlanChanged();
      },
    );

  const saveCopy = () => {
    const name = copyName.trim();
    if (!name) {
      submit.fail("Name the new scenario.");
      return;
    }
    let created: Scenario | undefined;
    submit.run(
      async () => {
        created = await api.whatIf.apply(scenario.id, {
          layers: (runnable ?? []).map((e) => e.layer),
          new_scenario_name: name,
        });
      },
      () => {
        setDialog(undefined);
        if (created) onScenarioCreated(created);
      },
    );
  };

  // ── drawing ──────────────────────────────────────────────────────────
  const stats = outcome ? whatIfStats(outcome) : undefined;
  const waterfall = useMemo(() => {
    if (!shown || !entries) return undefined;
    const views = new Map(entries.map((e) => [e.id, layerView(e, ctx)]));
    const names = shown.ranFor.map((id) => views.get(id)?.short ?? "removed");
    const titles = shown.ranFor.map((id) => {
      const v = views.get(id);
      return v ? v.parts.map((p) => (p.type === "text" ? p.text : p.value)).join(" ") : "removed";
    });
    return waterfallView(shown.outcome, names, titles);
  }, [shown, entries, ctx]);
  const fan = useMemo(() => (outcome ? fanView(outcome) : null), [outcome]);

  return (
    <>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 16,
          padding: "10px 20px",
          marginTop: 10,
          borderTop: "1px solid var(--color-divider)",
          borderBottom: "1px solid var(--color-divider)",
        }}
      >
        <span style={{ fontSize: 12, color: MUTED }}>
          {entries ? changedSummary(entries, ctx) : "Reading the stored overrides…"}
        </span>
        {/* Up here rather than above the charts: this row is always drawn, so
            the status coming and going never shifts the graphs. */}
        {outcome && (!current || refiningNow) && (
          <span
            style={{ fontSize: 12, color: MUTED, fontVariantNumeric: "tabular-nums" }}
            aria-live="polite"
          >
            {`${current ? "Refining" : "Updating"}…`}
            {current && job.active && job.job
              ? ` ${fmtInt(job.job.completed)} of about ${fmtInt(job.job.total)} simulations`
              : ""}
          </span>
        )}
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <Button
            variant="ghost"
            disabled={!entries || entries.length === 0}
            onClick={() => edit([])}
          >
            Reset
          </Button>
          <Button
            variant="secondary"
            disabled={!canWrite}
            title={canWrite ? undefined : "Switch an override on first"}
            onClick={() => {
              setCopyName(`${scenario.name} — what-if`);
              setDialog("save");
            }}
          >
            Save as scenario
          </Button>
          <Button
            variant="primary"
            disabled={!canWrite}
            title={canWrite ? undefined : "Switch an override on first"}
            onClick={() => setDialog("apply")}
          >
            Apply to plan
          </Button>
        </div>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "380px minmax(0, 1fr)" }}>
        <aside
          style={{
            padding: "16px 18px 18px",
            borderRight: "1px solid var(--color-divider)",
            display: "flex",
            flexDirection: "column",
            gap: 10,
          }}
        >
          <h6 style={{ margin: 0 }}>Overrides</h6>

          {entries?.map((entry) => {
            const view = layerView(entry, ctx);
            return (
              <OverrideCard
                key={entry.id}
                view={view}
                impact={impactText(entry, impacts, view.problem)}
                onToggle={() => patch(entry.id, (e) => ({ ...e, enabled: !e.enabled }))}
                onStep={(slot, dir) =>
                  patch(entry.id, (e) => ({ ...e, layer: stepLayer(e.layer, slot, dir, ctx) }))
                }
                onRemove={() => entries && edit(entries.filter((e) => e.id !== entry.id))}
              />
            );
          })}

          {tooMany && (
            <p role="alert" style={{ fontSize: 12, margin: 0 }}>
              At most {MAX_RUN_LAYERS} overrides can be on at once. Switch some off to measure
              the stack again.
            </p>
          )}

          <AddOverride
            options={entries ? addOptions(entries, ctx) : []}
            disabled={!entries}
            onAdd={add}
          />

          <p style={{ fontSize: 11.5, margin: "auto 0 0", color: MUTED, textWrap: "pretty" }}>
            Overrides never edit the plan. Plan inputs, market shocks and one-off events all
            stack the same way.
          </p>
        </aside>

        <div
          style={{
            padding: "16px 20px 20px",
            display: "flex",
            flexDirection: "column",
            gap: 14,
            minWidth: 0,
          }}
        >
          {quickError ? (
            <p role="alert" style={{ fontSize: 12.5, margin: 0 }}>
              The what-if did not finish: {quickError}
            </p>
          ) : (
            job.error && (
              <p style={{ fontSize: 12, margin: 0, color: MUTED }}>
                Refinement did not finish ({job.error}); showing the quick answer.
              </p>
            )
          )}
          {stack.error && (
            <p style={{ fontSize: 12, margin: 0, color: MUTED }}>
              The stored overrides could not be read ({stack.error}); starting from an empty
              stack.
            </p>
          )}

          {!outcome || !stats ? (
            !quickError && (
              <div style={{ padding: "24px 4px", fontSize: 13, color: MUTED }}>
                Measuring the plan…
              </div>
            )
          ) : (
            <div
              style={{
                display: "flex",
                flexDirection: "column",
                gap: 14,
                opacity: current ? 1 : 0.55,
                transition: "opacity 120ms",
              }}
              aria-busy={!current}
            >
              <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 16 }}>
                <Figure label="success" value={stats.success} delta={stats.successDelta} accent />
                <Figure label={stats.endLabel} value={stats.end} delta={stats.endDelta} />
                <Figure label="P10 runs dry" value={stats.dry} delta={stats.dryDelta} />
              </div>
              {waterfall && (
                <div>
                  <h6 style={{ margin: "0 0 6px" }}>
                    Where the success went{" "}
                    <span className="text-muted" style={{ letterSpacing: 0 }}>
                      each bar is one override, applied on top of the ones before it
                    </span>
                  </h6>
                  <SuccessWaterfall view={waterfall} />
                </div>
              )}

              {fan && (
                <div>
                  <h6 style={{ margin: "0 0 6px" }}>Net worth, today&apos;s dollars</h6>
                  <WhatIfFan view={fan} byAge={outcome.ages != null} />
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      {dialog === "apply" && (
        <Dialog
          title="Apply to plan"
          submitLabel="Apply"
          busy={submit.busy}
          error={submit.error}
          onClose={() => setDialog(undefined)}
          onSubmit={applyNow}
        >
          <p style={{ margin: 0, fontSize: 13.5, lineHeight: 1.5 }}>
            Write {lines.length === 1 ? "this change" : `these ${lines.length} changes`} into{" "}
            <span className="cd-name">{scenario.name}</span>? Overrides that are switched off are
            left out, and the stack is cleared afterwards.
          </p>
          <ChangeList lines={lines} />
        </Dialog>
      )}

      {dialog === "save" && (
        <Dialog
          title="Save as scenario"
          submitLabel="Save"
          busy={submit.busy}
          error={submit.error}
          onClose={() => setDialog(undefined)}
          onSubmit={saveCopy}
        >
          <Field label="Name">
            <Input
              aria-label="New scenario name"
              value={copyName}
              onChange={(e) => setCopyName(e.target.value)}
            />
          </Field>
          <p style={{ margin: 0, fontSize: 13.5, lineHeight: 1.5 }}>
            Copies <span className="cd-name">{scenario.name}</span> and writes{" "}
            {lines.length === 1 ? "this change" : `these ${lines.length} changes`} into the copy.
            This plan and its overrides stay as they are.
          </p>
          <ChangeList lines={lines} />
        </Dialog>
      )}
    </>
  );
}

function Figure({
  label,
  value,
  delta,
  accent,
}: {
  label: string;
  value: string;
  delta: string;
  accent?: boolean;
}) {
  return (
    <div>
      <div className="stat-l">{label}</div>
      <div className="stat-v" style={accent ? { color: "var(--color-accent-800)" } : undefined}>
        {value}
      </div>
      <div style={{ fontSize: 11.5, color: MUTED }}>{delta}</div>
    </div>
  );
}

function ChangeList({ lines }: { lines: string[] }) {
  return (
    <ul style={{ margin: 0, paddingLeft: 18, fontSize: 13, lineHeight: 1.5 }}>
      {lines.map((line, i) => (
        <li key={i}>{line}</li>
      ))}
    </ul>
  );
}

/**
 * The Add override menu: the plan's parameters not already overridden, then
 * the three event layers. A row that cannot be added says why on hover.
 */
function AddOverride({
  options,
  disabled,
  onAdd,
}: {
  options: AddOption[];
  disabled?: boolean;
  onAdd: (option: AddOption) => void;
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", onDown, true);
    window.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const parameters = options.filter((o) => o.choice.kind === "parameter");
  const events = options.filter((o) => o.choice.kind !== "parameter");

  const row = (option: AddOption) => (
    <button
      key={option.key}
      type="button"
      role="menuitem"
      className="dd-opt"
      aria-disabled={option.disabled != null || undefined}
      title={option.disabled ?? undefined}
      onClick={() => {
        if (option.disabled) return;
        onAdd(option);
        setOpen(false);
      }}
      style={{ width: "100%", border: 0, borderTop: "1px solid var(--color-divider)", background: "none", font: "inherit", fontSize: 13, textAlign: "left", color: "inherit" }}
    >
      <span className="mk" aria-hidden="true" />
      <span className="dd-label">{option.label}</span>
      {option.detail && <span className="sub">{option.detail}</span>}
    </button>
  );

  return (
    <div ref={root} className="dd" style={{ alignSelf: "flex-start" }} data-open={open ? "true" : undefined}>
      <Button
        variant="add"
        disabled={disabled}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        Add override
      </Button>
      {open && (
        <div
          className="dd-menu"
          role="menu"
          aria-label="Add override"
          style={{ right: "auto", width: "max-content", minWidth: 240, maxWidth: 340, borderTop: "1px solid var(--color-accent)", maxHeight: 360, overflowY: "auto" }}
        >
          <div className="dd-group" role="group" aria-label="Plan inputs">
            <div className="dd-head">Plan inputs</div>
            {parameters.length === 0 ? (
              <div className="dd-opt" aria-disabled="true" style={{ fontSize: 12 }}>
                <span className="mk" aria-hidden="true" />
                <span className="dd-label">
                  {options.length === 0 ? "…" : "No named parameters left to override"}
                </span>
              </div>
            ) : (
              parameters.map(row)
            )}
          </div>
          <div className="dd-group" role="group" aria-label="Events">
            <div className="dd-head">Events</div>
            {events.map(row)}
          </div>
        </div>
      )}
    </div>
  );
}
