"use client";

import { type CSSProperties, Fragment, type ReactNode, useState } from "react";
import { Button, Dialog, Field, Input, NumberInput } from "@/components/ui";
import type { TaxConfig } from "@/lib/api/types";
import { useAsync } from "@/lib/hooks/useAsync";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { usePlanApi } from "@/lib/nav";
import {
  type BracketDraft,
  type TaxDraft,
  NIIT_DEFAULT_THRESHOLD,
  bracketProblem,
  draftProblem,
  fromTaxDraft,
  sameDraft,
  scheduleChart,
  toTaxDraft,
  yearTax,
} from "@/lib/view/taxSchedule";

/** What the dialog opened on: a config to edit, or a new one to start. */
export type TaxConfigTarget =
  | { kind: "edit"; config: TaxConfig }
  /** `from`'s figures are the starting point; absent, one 10% bracket. */
  | { kind: "new"; from?: TaxConfig };

const MUTED = "color-mix(in srgb, var(--color-text) 62%, transparent)";
const FAINT = "color-mix(in srgb, var(--color-text) 48%, transparent)";
const HATCH =
  "repeating-linear-gradient(135deg, var(--color-surface) 0 4px, var(--color-neutral-200) 4px 8px)";
const GAINS = "var(--color-series-2)";

/** A rate's band in the chart: the higher the rate, the deeper the blue. */
function shade(rate: number | null): string {
  if (rate == null) return HATCH;
  return `color-mix(in srgb, var(--color-accent) ${Math.round(7 + rate * 1.25)}%, var(--color-raised))`;
}

const dollars = (n: number) => `$${Math.round(n).toLocaleString("en-US")}`;

/** The 2024 long-term gains schedules, for a config migrated from a flat rate. */
const GAINS_2024: Record<"single" | "joint", BracketDraft[]> = {
  single: [
    { threshold: 0, rate: 0 },
    { threshold: 47_025, rate: 15 },
    { threshold: 518_900, rate: 20 },
  ],
  joint: [
    { threshold: 0, rate: 0 },
    { threshold: 94_050, rate: 15 },
    { threshold: 583_750, rate: 20 },
  ],
};

function blankDraft(): TaxDraft {
  return {
    name: "",
    description: null,
    stateRate: 0,
    penaltyRate: 10,
    standardDeduction: 0,
    age65Extra: 0,
    niitThreshold: null,
    federal: [{ threshold: 0, rate: 10 }],
    gains: [{ threshold: 0, rate: 0 }],
  };
}

function startingDraft(target: TaxConfigTarget): TaxDraft {
  if (target.kind === "edit") return toTaxDraft(target.config);
  if (!target.from) return blankDraft();
  return { ...toTaxDraft(target.from), name: "", description: null };
}

/**
 * Canvas 1a — the tax config editor, opened from the Plan strip's Tax config
 * field ("Edit…", or "New tax config…" at the foot of its picker).
 *
 * A tax config is shared across scenarios like a return profile, so the
 * dialog says so up front and lists who uses it. The brackets sit beside what
 * they do: a chart of where each dollar of gross income lands, and a worked
 * year that stacks long-term gains on the ordinary income. Both redraw on
 * every keystroke; nothing is written until Apply.
 */
export function TaxConfigDialog({
  target: opened,
  onClose,
  onSaved,
  onDeleted,
  offline,
}: {
  target: TaxConfigTarget;
  onClose: () => void;
  /** After a write; `created` when it was a new config. A rejection shows as the save's error. */
  onSaved: (config: TaxConfig, created: boolean) => void | Promise<void>;
  onDeleted: () => void;
  offline?: boolean;
}) {
  const api = usePlanApi();
  // Duplicate turns an edit into a new config without closing the dialog.
  const [target, setTarget] = useState(opened);
  const [draft, setDraft] = useState(() => startingDraft(opened));
  const [pristine, setPristine] = useState(draft);
  const [example, setExample] = useState({ ordinary: 90_000, gains: 60_000 });
  const [status, setStatus] = useState<string>();
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const submit = useSubmit();

  const editing = target.kind === "edit" ? target.config : undefined;
  const scenarios = useAsync(() => api.scenarios.list(), [api]);
  const usedBy = editing
    ? (scenarios.data ?? []).filter((s) => s.tax_config_id === editing.id)
    : [];

  const dirty = target.kind === "new" || !sameDraft(draft, pristine);
  const problem = draftProblem(draft);
  const canSave = dirty && !problem && !offline && !submit.busy;

  const change = (patch: Partial<TaxDraft>) => {
    setDraft((d) => ({ ...d, ...patch }));
    setStatus(undefined);
  };

  const save = () => {
    if (!canSave) {
      if (problem) submit.fail(problem);
      return;
    }
    const created = !editing;
    submit.run(
      async () => {
        const saved = editing
          ? await api.taxConfigs.update(editing.id, fromTaxDraft(draft))
          : await api.taxConfigs.create(fromTaxDraft(draft));
        // Edit mode from here on, so a failure below cannot create it twice.
        const next = toTaxDraft(saved);
        setTarget({ kind: "edit", config: saved });
        setDraft(next);
        setPristine(next);
        await onSaved(saved, created);
      },
      () => {
        scenarios.reload();
        setStatus(
          created
            ? "Created, and this scenario now uses it."
            : "Saved. Every scenario using it reruns with it next time.",
        );
      },
    );
  };

  const remove = () => {
    if (!editing) return;
    submit.run(
      () => api.taxConfigs.remove(editing.id),
      () => {
        onDeleted();
        onClose();
      },
    );
  };

  const duplicate = () => {
    if (!editing) return;
    setTarget({ kind: "new", from: editing });
    setDraft({ ...draft, name: `${draft.name.trim()} (copy)`, description: null });
    setStatus("A copy: change it, then Create. The original is left as it is.");
  };

  const revert = () => {
    setDraft(pristine);
    setStatus(undefined);
    submit.fail("");
  };

  const chart = scheduleChart(draft, example);
  const year = yearTax(draft, example.ordinary, example.gains);
  const flatGains = draft.gains.length === 1 && draft.gains[0].rate > 0;

  const footer = confirmingDelete && editing ? (
    <FooterRow key="confirm">
      <span style={{ marginRight: "auto", fontSize: 13, lineHeight: 1.45 }}>
        Delete <b>{editing.name}</b>?{" "}
        {usedBy.length > 0
          ? `${usedBy.length === 1 ? "1 scenario uses" : `${usedBy.length} scenarios use`} it and will run with no taxes until given another.`
          : "No scenario uses it."}
      </span>
      <Button onClick={() => setConfirmingDelete(false)}>Keep it</Button>
      <Button variant="danger" onClick={remove} disabled={submit.busy || offline}>
        {submit.busy ? "…" : "Delete"}
      </Button>
    </FooterRow>
  ) : (
    <FooterRow key="actions">
      {editing && (
        <>
          <span className="stat-l" style={{ margin: 0 }}>
            Used by
          </span>
          {usedBy.length === 0 ? (
            <span style={{ fontSize: 12.5, color: MUTED }}>
              {scenarios.data ? "no scenario" : "…"}
            </span>
          ) : (
            usedBy.map((s) => (
              <span
                key={s.id}
                style={{
                  padding: "1px 10px",
                  background: "var(--color-surface)",
                  borderRadius: 999,
                  fontSize: 12.5,
                }}
              >
                {s.name}
              </span>
            ))
          )}
        </>
      )}
      <span style={{ marginRight: "auto", fontSize: 12.5, color: MUTED }}>
        {status ?? (problem && dirty ? problem : dirty && editing ? "Unsaved changes." : "")}
      </span>
      {/* Keyed apart: Duplicate flips the dialog to "new", and a reused
          button turned type=submit mid-click would submit the form. */}
      {editing ? (
        <Fragment key="edit">
          <Button variant="danger" onClick={() => setConfirmingDelete(true)} disabled={offline}>
            Delete…
          </Button>
          <Button onClick={duplicate} disabled={offline}>
            Duplicate
          </Button>
          {dirty && <Button onClick={revert}>Revert</Button>}
          <Button type="submit" variant="primary" disabled={!canSave}>
            {submit.busy ? "…" : "Apply"}
          </Button>
        </Fragment>
      ) : (
        <Fragment key="new">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={!canSave}>
            {submit.busy ? "…" : "Create"}
          </Button>
        </Fragment>
      )}
    </FooterRow>
  );

  return (
    <Dialog
      title={editing ? "Tax config" : "New tax config"}
      aside={
        <>
          <span style={{ color: MUTED, fontSize: 13.5, marginRight: "auto" }}>
            {editing
              ? "Shared: editing it changes every scenario that uses it."
              : `${target.kind === "new" && target.from ? `Starts from ${target.from.name}. ` : ""}Added to your library, and this scenario switches to it.`}
          </span>
          <Button variant="ghost" onClick={onClose}>
            Close
          </Button>
        </>
      }
      width={1080}
      onClose={onClose}
      onSubmit={save}
      footer={footer}
      error={submit.error || undefined}
    >
      <div
        className="mobile-stack"
        style={{
          display: "grid",
          gridTemplateColumns: "380px minmax(0, 1fr)",
          gap: 28,
          paddingTop: 12,
          borderTop: "1px solid var(--color-divider)",
        }}
      >
        <div style={{ display: "flex", flexDirection: "column", gap: 18, minWidth: 0 }}>
          <Field label="Name">
            <Input
              aria-label="Tax config name"
              value={draft.name}
              readOnly={offline}
              onChange={(e) => change({ name: e.target.value })}
            />
          </Field>

          <Section title="Ordinary income · federal">
            <Brackets
              label="Ordinary"
              rows={draft.federal}
              readOnly={offline}
              onChange={(federal) => change({ federal })}
            />
            <InlineMoney
              label="Standard deduction"
              value={draft.standardDeduction}
              readOnly={offline}
              onChange={(standardDeduction) => change({ standardDeduction })}
            />
            <InlineMoney
              label="Extra deduction from 65"
              value={draft.age65Extra}
              readOnly={offline}
              onChange={(age65Extra) => change({ age65Extra })}
            />
          </Section>

          <Section title="Long-term capital gains">
            {flatGains && (
              <div
                style={{
                  display: "flex",
                  flexDirection: "column",
                  gap: 4,
                  padding: "10px 12px",
                  background: `color-mix(in srgb, ${GAINS} 10%, transparent)`,
                  border: `1px solid color-mix(in srgb, ${GAINS} 35%, transparent)`,
                  borderRadius: 8,
                  fontSize: 13,
                }}
              >
                <span style={{ textWrap: "pretty" }}>
                  One flat rate: {draft.gains[0].rate}% on every taxable gain.
                </span>
                {!offline && (
                  <span style={{ display: "flex", gap: 4 }}>
                    <Button variant="ghost" onClick={() => change({ gains: GAINS_2024.single })}>
                      Use the 2024 single schedule
                    </Button>
                    <Button variant="ghost" onClick={() => change({ gains: GAINS_2024.joint })}>
                      Use the 2024 joint schedule
                    </Button>
                  </span>
                )}
              </div>
            )}
            <Brackets
              label="Long-term gains"
              rows={draft.gains}
              readOnly={offline}
              onChange={(gains) => change({ gains })}
            />
            <Hint>
              Gains sit on top of ordinary income, so these thresholds are total taxable income.
              Short-term gains count as ordinary income for the rest of the year.
            </Hint>
          </Section>

          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
              <Switch
                label="Net investment income tax"
                on={draft.niitThreshold != null}
                disabled={offline}
                onToggle={(on) => change({ niitThreshold: on ? NIIT_DEFAULT_THRESHOLD : null })}
              />
              <span style={{ fontWeight: 600 }}>Net investment income tax</span>
              <span style={{ marginLeft: "auto", color: MUTED }}>3.8%</span>
            </div>
            {draft.niitThreshold != null && (
              <>
                <InlineMoney
                  label="Income threshold (MAGI)"
                  value={draft.niitThreshold}
                  readOnly={offline}
                  onChange={(niitThreshold) => change({ niitThreshold })}
                />
                <Hint>
                  Charged on the gains or on income over the threshold, whichever is smaller. The
                  threshold is not indexed to inflation.
                </Hint>
              </>
            )}
          </div>
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 20, minWidth: 0 }}>
          <ScheduleChartView chart={chart} example={example} niit={draft.niitThreshold != null} />

          <div
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 10,
              paddingTop: 16,
              borderTop: "1px solid var(--color-divider)",
            }}
          >
            <span className="stat-l" style={{ margin: 0 }}>
              Try a year
            </span>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: 12 }}>
              <Field label="Ordinary income">
                <NumberInput
                  prefix="$"
                  decimals={0}
                  value={example.ordinary}
                  aria-label="Example ordinary income"
                  onValueChange={(ordinary) => setExample((e) => ({ ...e, ordinary }))}
                />
              </Field>
              <Field label="Long-term gains">
                <NumberInput
                  prefix="$"
                  decimals={0}
                  value={example.gains}
                  aria-label="Example long-term gains"
                  onValueChange={(gains) => setExample((e) => ({ ...e, gains }))}
                />
              </Field>
            </div>
            <div style={{ display: "flex", flexDirection: "column", fontSize: 13.5 }}>
              <div style={{ ...PIECE_ROW, paddingBottom: 4, borderBottom: "1px solid var(--color-text)", fontSize: 12, color: MUTED }}>
                <span>Gains taxed at</span>
                <span style={{ textAlign: "right" }}>Amount</span>
                <span style={{ textAlign: "right" }}>Tax</span>
              </div>
              {year.pieces.length === 0 && (
                <div style={{ ...PIECE_ROW, padding: "6px 0", color: MUTED }}>
                  <span>No gains this year.</span>
                </div>
              )}
              {year.pieces.map((p) => (
                <div
                  key={p.label}
                  style={{ ...PIECE_ROW, padding: "6px 0", borderBottom: "1px solid var(--color-rule)" }}
                >
                  <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                    <i style={{ width: 10, height: 10, borderRadius: 2, background: shade(p.rate) }} />
                    {p.label}
                  </span>
                  <span style={{ textAlign: "right" }}>{dollars(p.amount)}</span>
                  <span style={{ textAlign: "right" }}>{dollars(p.tax)}</span>
                </div>
              ))}
              <div style={{ ...PIECE_ROW, paddingTop: 8, fontWeight: 600 }}>
                <span>Federal tax on the gains</span>
                <span style={{ textAlign: "right", fontWeight: 400, color: MUTED }}>
                  {example.gains > 0
                    ? `${((year.gainsTax / example.gains) * 100).toFixed(1)}% effective`
                    : "—"}
                </span>
                <span style={{ textAlign: "right" }}>{dollars(year.gainsTax)}</span>
              </div>
              <span style={{ marginTop: 6, fontSize: 12.5, color: MUTED }}>
                Ordinary income alone: {dollars(year.ordinaryTax)} federal, {year.marginal}% marginal.
              </span>
            </div>
          </div>

          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
              gap: 12,
              paddingTop: 16,
              borderTop: "1px solid var(--color-divider)",
            }}
          >
            <Field label="State income tax, flat">
              <NumberInput
                suffix="%"
                group={false}
                min={0}
                max={100}
                value={draft.stateRate}
                readOnly={offline}
                aria-label="State income tax rate"
                onValueChange={(stateRate) => change({ stateRate })}
              />
            </Field>
            <Field label="Early withdrawal penalty, before 59½">
              <NumberInput
                suffix="%"
                group={false}
                min={0}
                max={100}
                value={draft.penaltyRate}
                readOnly={offline}
                aria-label="Early withdrawal penalty rate"
                onValueChange={(penaltyRate) => change({ penaltyRate })}
              />
            </Field>
          </div>

          <Hint>
            Pre-tax money left when the plan ends is valued after the tax these brackets would
            charge to withdraw it evenly over ten years — the deadline most heirs have — on top of
            the plan’s last full year of income. That is the after-tax ending balance Drawdown
            compares strategies on.
          </Hint>
        </div>
      </div>
    </Dialog>
  );
}

const BRACKET_ROW: CSSProperties = {
  display: "grid",
  gridTemplateColumns: "minmax(0, 1fr) 96px 24px",
  gap: 8,
  alignItems: "center",
};

const PIECE_ROW: CSSProperties = {
  display: "grid",
  gridTemplateColumns: "minmax(0, 1fr) 110px 90px",
  gap: 8,
};

/** A bracket table: threshold and rate per row, the first fixed at $0. */
function Brackets({
  label,
  rows,
  readOnly,
  onChange,
}: {
  label: string;
  rows: BracketDraft[];
  readOnly?: boolean;
  onChange: (rows: BracketDraft[]) => void;
}) {
  const edit = (i: number, patch: Partial<BracketDraft>) =>
    onChange(rows.map((row, j) => (j === i ? { ...row, ...patch } : row)));
  const add = () => {
    const last = rows[rows.length - 1] ?? { threshold: -50_000, rate: 0 };
    onChange([...rows, { threshold: last.threshold + 50_000, rate: last.rate }]);
  };
  const problem = bracketProblem(rows);

  return (
    <>
      <div
        style={{
          ...BRACKET_ROW,
          paddingBottom: 4,
          borderBottom: "1px solid var(--color-text)",
          fontSize: 12,
          color: MUTED,
        }}
      >
        <span>Taxable income from</span>
        <span style={{ textAlign: "right" }}>Rate</span>
        <span />
      </div>
      {rows.map((row, i) => (
        <div key={i} style={BRACKET_ROW}>
          <NumberInput
            prefix="$"
            decimals={0}
            value={row.threshold}
            readOnly={readOnly || i === 0}
            aria-label={`${label} bracket ${i + 1} threshold`}
            onValueChange={(threshold) => edit(i, { threshold })}
          />
          <NumberInput
            suffix="%"
            group={false}
            min={0}
            max={100}
            value={row.rate}
            readOnly={readOnly}
            aria-label={`${label} bracket ${i + 1} rate`}
            onValueChange={(rate) => edit(i, { rate })}
          />
          {i > 0 && !readOnly ? (
            <button
              type="button"
              className="row-delete"
              title="Remove bracket"
              aria-label={`Remove ${label.toLowerCase()} bracket ${i + 1}`}
              onClick={() => onChange(rows.filter((_, j) => j !== i))}
            >
              ×
            </button>
          ) : (
            <span />
          )}
        </div>
      ))}
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 8 }}>
        {!readOnly ? (
          <Button variant="ghost" onClick={add} style={{ marginLeft: -8 }}>
            Add bracket
          </Button>
        ) : (
          <span />
        )}
        {problem && <span style={{ fontSize: 12, color: "var(--color-danger)" }}>{problem}</span>}
      </div>
    </>
  );
}

/** A dollar figure on one line with its label, as the deductions are. */
function InlineMoney({
  label,
  value,
  readOnly,
  onChange,
}: {
  label: string;
  value: number;
  readOnly?: boolean;
  onChange: (value: number) => void;
}) {
  return (
    <label
      style={{
        display: "grid",
        gridTemplateColumns: "minmax(0, 1fr) 150px",
        gap: 8,
        alignItems: "center",
      }}
    >
      <span style={{ fontSize: 13, color: MUTED, fontWeight: 500 }}>{label}</span>
      <NumberInput
        prefix="$"
        decimals={0}
        value={value}
        readOnly={readOnly}
        aria-label={label}
        onValueChange={onChange}
      />
    </label>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <span className="stat-l" style={{ margin: 0 }}>
        {title}
      </span>
      {children}
    </div>
  );
}

function Hint({ children }: { children: ReactNode }) {
  return <p style={{ margin: 0, fontSize: 12, lineHeight: 1.45, color: MUTED, textWrap: "pretty" }}>{children}</p>;
}

function FooterRow({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        flexWrap: "wrap",
        gap: 10,
        marginTop: 4,
        paddingTop: 14,
        borderTop: "1px solid var(--color-divider)",
      }}
    >
      {children}
    </div>
  );
}

function Switch({
  label,
  on,
  disabled,
  onToggle,
}: {
  label: string;
  on: boolean;
  disabled?: boolean;
  onToggle: (on: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      onClick={() => onToggle(!on)}
      style={{
        flex: "none",
        position: "relative",
        width: 34,
        height: 20,
        padding: 0,
        border: 0,
        borderRadius: 999,
        cursor: disabled ? "default" : "pointer",
        background: on ? "var(--color-accent)" : "var(--color-neutral-300)",
        transition: "background .15s",
      }}
    >
      <i
        style={{
          position: "absolute",
          top: 2,
          left: on ? 16 : 2,
          width: 16,
          height: 16,
          borderRadius: "50%",
          background: "#fff",
          boxShadow: "0 1px 2px rgba(0,0,0,.25)",
          transition: "left .15s",
        }}
      />
    </button>
  );
}

const pct = (n: number) => `${n.toFixed(3)}%`;

/**
 * Where each dollar lands: the ordinary brackets, the gains schedule and NIIT
 * as three bands on one gross-income axis, the deduction hatched, with the
 * example year's ordinary income marked and its gains outlined on top.
 */
function ScheduleChartView({
  chart,
  example,
  niit,
}: {
  chart: ReturnType<typeof scheduleChart>;
  example: { ordinary: number; gains: number };
  niit: boolean;
}) {
  const band = (segments: ReturnType<typeof scheduleChart>["ordinary"], overlay?: ReactNode) => (
    <div
      style={{
        position: "relative",
        height: 32,
        borderRadius: 4,
        overflow: "hidden",
        background: "var(--color-raised)",
        boxShadow: "inset 0 0 0 1px var(--color-divider)",
      }}
    >
      {segments.map((g, i) => (
        <div
          key={i}
          title={g.title}
          style={{
            position: "absolute",
            top: 0,
            bottom: 0,
            left: pct(g.left),
            width: pct(g.width),
            background: shade(g.rate),
            borderRight: "1px solid var(--color-raised)",
            display: "grid",
            placeItems: "center",
            fontSize: 11.5,
            fontWeight: 600,
            overflow: "hidden",
          }}
        >
          {g.rate != null && g.width > 4.2 ? `${Number(g.rate.toFixed(2))}%` : ""}
        </div>
      ))}
      {overlay}
    </div>
  );
  const ordinaryAt = chart.at(example.ordinary);
  const gainsTo = chart.at(example.ordinary + example.gains);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <div style={{ display: "flex", alignItems: "baseline", flexWrap: "wrap", gap: 8 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 600, fontSize: 19 }}>
          Where each dollar lands
        </span>
        <span style={{ fontFamily: "var(--font-display)", color: MUTED, fontSize: 15 }}>
          gross income, today’s dollars
        </span>
      </div>
      <div style={{ display: "grid", gridTemplateColumns: "112px minmax(0, 1fr)", gap: "0 12px" }}>
        <div style={{ display: "flex", flexDirection: "column", gap: 8, fontSize: 13, fontWeight: 600 }}>
          <span style={{ height: 32, display: "flex", alignItems: "center" }}>Ordinary</span>
          <span style={{ height: 32, display: "flex", alignItems: "center" }}>Long-term gains</span>
          <span style={{ height: 32, display: "flex", alignItems: "center", opacity: niit ? 1 : 0.35 }}>
            NIIT
          </span>
        </div>
        <div style={{ position: "relative", display: "flex", flexDirection: "column", gap: 8 }}>
          {band(chart.ordinary)}
          {band(
            chart.gains,
            <div
              style={{
                position: "absolute",
                top: 2,
                bottom: 2,
                left: pct(ordinaryAt),
                width: pct(Math.max(0.6, gainsTo - ordinaryAt)),
                border: `2px solid ${GAINS}`,
                background: `color-mix(in srgb, ${GAINS} 20%, transparent)`,
                borderRadius: 3,
                boxSizing: "border-box",
                transition: "left .2s, width .2s",
              }}
            />,
          )}
          <div style={{ opacity: niit ? 1 : 0.35 }}>{band(chart.niit)}</div>
          <i
            aria-hidden="true"
            style={{
              position: "absolute",
              top: -6,
              height: 32 * 3 + 8 * 2 + 12,
              left: pct(ordinaryAt),
              borderLeft: "2px solid var(--color-text)",
              transition: "left .2s",
            }}
          />
          <div style={{ position: "relative", height: 16 }}>
            {chart.ticks.map((t) => (
              <span
                key={t.label}
                style={{
                  position: "absolute",
                  left: pct(t.left),
                  transform:
                    t.align === "start" ? "none" : t.align === "end" ? "translateX(-100%)" : "translateX(-50%)",
                  fontSize: 11.5,
                  color: FAINT,
                  whiteSpace: "nowrap",
                }}
              >
                {t.label}
              </span>
            ))}
          </div>
        </div>
      </div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: "6px 18px", fontSize: 12.5, color: MUTED }}>
        <Legend swatch={<i style={{ width: 12, height: 12, borderRadius: 2, background: HATCH }} />}>
          Standard deduction, 0%
        </Legend>
        <Legend swatch={<i style={{ width: 2, height: 14, background: "var(--color-text)" }} />}>
          Ordinary income ends
        </Legend>
        <Legend
          swatch={
            <i
              style={{
                width: 14,
                height: 10,
                border: `2px solid ${GAINS}`,
                background: `color-mix(in srgb, ${GAINS} 20%, transparent)`,
                borderRadius: 2,
                boxSizing: "border-box",
              }}
            />
          }
        >
          Long-term gains, stacked on top
        </Legend>
        <span>Thresholds rise each year with the scenario’s inflation.</span>
      </div>
    </div>
  );
}

function Legend({ swatch, children }: { swatch: ReactNode; children: ReactNode }) {
  return (
    <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
      {swatch}
      {children}
    </span>
  );
}
