"use client";

import { Blueprint, Field, NumberInput, PercentInput, Select, StatLabel } from "@/components/ui";
import type { HistoryPreset } from "@/lib/api/types";
import { historyStats } from "@/lib/history";
import type { DistributionKind } from "@/lib/types";
import { pct } from "./distribution";
import type { DistributionDraft } from "./distributionDraft";

/** Every kind, in the order the select offers them: simplest first. */
export const DISTRIBUTIONS: DistributionKind[] = [
  "None",
  "Fixed",
  "Normal",
  "LogNormal",
  "StudentT",
  "RegimeSwitching",
  "Bootstrap",
];

/** The names people use, where they differ from the enum's. */
export const KIND_LABEL: Record<DistributionKind, string> = {
  None: "None",
  Fixed: "Fixed",
  Normal: "Normal",
  LogNormal: "LogNormal",
  StudentT: "Student-t",
  RegimeSwitching: "Regime switching",
  Bootstrap: "Bootstrap",
};

/** Common df choices; 3 is the fat-tail default and 30 converges on a normal. */
const DF_CHOICES = [3, 5, 10, 30];

const PAIR = { display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 } as const;

/** Every draft field a rate control edits — all of them percent per year. */
type PercentKey =
  | "rate"
  | "mean"
  | "sd"
  | "scale"
  | "bullMean"
  | "bullSd"
  | "bearMean"
  | "bearSd"
  | "bullToBear"
  | "bearToBull";

/**
 * The terms one distribution kind takes, and nothing else.
 *
 * A parameter the kind does not take is absent rather than disabled: a greyed
 * Volatility on a Fixed profile suggests a number is being ignored, when in
 * fact the profile has no such number. Switching kind swaps only this block —
 * the name, the description and what uses the profile are untouched by it.
 */
export function DistributionTerms({
  draft,
  onChange,
  presets,
  readOnly,
}: {
  draft: DistributionDraft;
  onChange: (patch: Partial<DistributionDraft>) => void;
  /** The histories the server offers; only Bootstrap reads them. */
  presets: HistoryPreset[];
  readOnly?: boolean;
}) {
  const percent = (label: string, key: PercentKey) => (
    <Field label={label}>
      <PercentInput
        style={{ minHeight: 32 }}
        value={draft[key]}
        readOnly={readOnly}
        // A computed key off a union widens to `string`, which `Partial` will
        // not take; the union itself is the guarantee the cast stands on.
        onValueChange={(v) => onChange({ [key]: v } as Partial<DistributionDraft>)}
        aria-label={label}
      />
    </Field>
  );

  switch (draft.kind) {
    case "None":
      return (
        <p
          style={{
            margin: 0,
            fontSize: 12.5,
            lineHeight: 1.6,
            color: "color-mix(in srgb, var(--color-text) 62%, transparent)",
          }}
        >
          No parameters. The balance is carried forward untouched — the row exists
          so a property or a loan can point at something explicit instead of at a
          0% Fixed.
        </p>
      );

    case "Fixed":
      return percent("Annual rate", "rate");

    case "Normal":
      return (
        <div style={PAIR}>
          {percent("Mean return", "mean")}
          {percent("Volatility", "sd")}
        </div>
      );

    case "LogNormal":
      // The parameters are the log's, so the labels have to say so: the figure
      // typed here is the median multiplier, and the arithmetic mean is higher.
      return (
        <div style={PAIR}>
          {percent("Median return", "mean")}
          {percent("Log volatility", "sd")}
        </div>
      );

    case "StudentT":
      return (
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 62px", gap: 10 }}>
          {percent("Mean", "mean")}
          {percent("Scale", "scale")}
          <Field label="df">
            <Select
              style={{ minHeight: 32 }}
              value={draft.df}
              disabled={readOnly}
              onChange={(e) => onChange({ df: Number(e.target.value) })}
              aria-label="Degrees of freedom"
            >
              {(DF_CHOICES.includes(draft.df) ? DF_CHOICES : [draft.df, ...DF_CHOICES]).map(
                (df) => (
                  <option key={df} value={df}>
                    {df}
                  </option>
                ),
              )}
            </Select>
          </Field>
        </div>
      );

    case "RegimeSwitching":
      return (
        <div style={PAIR}>
          {percent("Bull mean", "bullMean")}
          {percent("Bull volatility", "bullSd")}
          {percent("Bear mean", "bearMean")}
          {percent("Bear volatility", "bearSd")}
          {percent("Bull → bear / yr", "bullToBear")}
          {percent("Bear → bull / yr", "bearToBull")}
        </div>
      );

    case "Bootstrap": {
      const chosen = presets.find((p) => p.id === draft.preset);
      const stats = chosen ? historyStats(chosen.returns) : null;
      return (
        <>
          <Field label="Historical preset">
            <Select
              style={{ minHeight: 32 }}
              value={draft.preset}
              disabled={readOnly}
              onChange={(e) => onChange({ preset: e.target.value })}
            >
              {draft.preset === "" && <option value="">Pick a history…</option>}
              {/* A preset the server no longer offers still has to be showable,
                  or opening the profile would silently remap it. */}
              {!chosen && draft.preset !== "" && (
                <option value={draft.preset}>{draft.preset}</option>
              )}
              {presets.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Block size">
            <NumberInput
              style={{ minHeight: 32 }}
              nullable
              value={draft.blockSize}
              decimals={0}
              min={1}
              placeholder="1"
              affixesWhenEmpty
              readOnly={readOnly}
              onValueChange={(v) => onChange({ blockSize: v })}
              suffix={(draft.blockSize ?? 1) === 1 ? "year" : "years"}
              aria-label="Block size"
            />
          </Field>
          <Blueprint
            style={{
              padding: "8px 10px",
              background: "color-mix(in srgb, var(--color-accent) 6%, transparent)",
            }}
          >
            <StatLabel>drawn from data</StatLabel>
            <div style={{ fontSize: 11.5, marginTop: 3, lineHeight: 1.5 }}>
              {chosen && stats ? (
                <>
                  {stats.years} observed years, {chosen.start_year}&ndash;
                  {chosen.start_year + stats.years - 1} &middot; mean {pct(stats.mean)},
                  &sigma; {pct(stats.sd)} &middot; worst {pct(stats.min)}, best{" "}
                  {pct(stats.max)}. Resampled in blocks, so a bad year keeps the
                  year that followed it. Nothing here is fitted: the shape above
                  is those years, counted.
                </>
              ) : (
                <>
                  The shape is the history itself, resampled in blocks so a bad
                  year keeps the year that followed it — pick one and its years
                  are drawn above.
                </>
              )}
            </div>
          </Blueprint>
        </>
      );
    }
  }
}
