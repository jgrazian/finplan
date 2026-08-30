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
  "Fixed",
  "Normal",
  "LogNormal",
  "Bootstrap",
  "Historical",
];

/**
 * Inspects the selected return profile. Historical and Bootstrap presets are
 * sampled from real series, so their parameters are read-only — duplicating
 * one yields an editable copy.
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
          disabled={readOnly}
          onChange={(e) => onKindChange?.(e.target.value as DistributionKind)}
        >
          {DISTRIBUTIONS.map((d) => (
            <option key={d}>{d}</option>
          ))}
        </Select>
      </Field>

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
        <Field label="Mean return">
          <CompactInput value={pct(profile.mean)} readOnly={readOnly} />
        </Field>
        <Field label="Volatility">
          <CompactInput
            value={profile.sd === 0 ? "0" : pct(profile.sd)}
            readOnly={readOnly}
          />
        </Field>
      </div>

      <Field label="Sample source">
        <CompactInput value={profile.source} readOnly={readOnly} />
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
