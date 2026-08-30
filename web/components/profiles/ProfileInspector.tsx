"use client";

import {
  Button,
  CompactInput,
  Field,
  Hr,
  SectionHeading,
  Select,
  Tag,
} from "@/components/ui";
import type { DistributionKind, ReturnProfile } from "@/lib/types";
import { DistributionCurve } from "./DistributionCurve";
import { pct } from "./distribution";

const DISTRIBUTIONS: DistributionKind[] = [
  "None",
  "Fixed",
  "Normal",
  "LogNormal",
  "StudentT",
  "RegimeSwitching",
  "Bootstrap",
];

/**
 * Inspects the selected return profile.
 *
 * The parameter fields display rather than edit: nothing here is wired to
 * `PATCH /return-profiles/{id}` yet, so they are read-only for every profile.
 * `readOnly` is the narrower statement that this profile is a sampled preset,
 * whose shape comes from data and could not be typed in even once editing
 * lands — duplicating one yields a parameterised copy.
 */
export function ProfileInspector({
  profile,
  readOnly,
  onKindChange,
  onDuplicate,
  onApply,
}: {
  profile: ReturnProfile;
  readOnly?: boolean;
  onKindChange?: (kind: DistributionKind) => void;
  onDuplicate?: () => void;
  onApply?: () => void;
}) {
  return (
    <div
      style={{
        padding: "16px 18px 20px",
        display: "flex",
        flexDirection: "column",
        gap: 12,
        height: "100%",
      }}
    >
      <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between" }}>
        <h5 style={{ margin: 0, fontFamily: "ui-monospace, Menlo, monospace", fontSize: 14 }}>
          {profile.id}
        </h5>
        <Tag tone="outline">{profile.kind}</Tag>
      </div>

      <Field label="Distribution">
        <Select
          style={{ minHeight: 32 }}
          value={profile.kind}
          disabled={readOnly || !onKindChange}
          onChange={(e) => onKindChange?.(e.target.value as DistributionKind)}
        >
          {DISTRIBUTIONS.map((d) => (
            <option key={d}>{d}</option>
          ))}
        </Select>
      </Field>

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
        <Field label="Mean return">
          <CompactInput value={pct(profile.mean)} readOnly />
        </Field>
        <Field label="Volatility">
          <CompactInput value={profile.sd === 0 ? "0" : pct(profile.sd)} readOnly />
        </Field>
      </div>

      <Field label="Sample source">
        <CompactInput value={profile.source} readOnly />
      </Field>

      <DistributionCurve kind={profile.kind} mean={profile.mean} sd={profile.sd} />

      <Hr flush />

      <div>
        <SectionHeading className="mb-[6px]">Assets on this profile</SectionHeading>
        <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
          {profile.usedBy.length === 0 ? (
            <span className="text-muted" style={{ fontSize: 12 }}>
              No assets reference this profile.
            </span>
          ) : (
            profile.usedBy.map((u) => (
              <Tag key={u} tone="neutral">
                {u}
              </Tag>
            ))
          )}
        </div>
      </div>

      <div style={{ display: "flex", gap: 8, marginTop: "auto" }}>
        <Button style={{ flex: 1 }} onClick={onDuplicate}>
          Duplicate
        </Button>
        <Button variant="primary" style={{ flex: 1 }} onClick={onApply} disabled={readOnly}>
          Apply
        </Button>
      </div>
    </div>
  );
}
