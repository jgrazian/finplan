"use client";

import { useState } from "react";
import {
  Button,
  CompactInput,
  Field,
  Hr,
  SectionHeading,
  Select,
  Tag,
} from "@/components/ui";
import type { AssetClass, HistoryPreset, UpdateProfile } from "@/lib/api/types";
import { ASSET_CLASSES, CLASS_LABEL } from "@/lib/tickers";
import type { DistributionKind, ReturnProfile } from "@/lib/types";
import { DISTRIBUTIONS, DistributionTerms, KIND_LABEL } from "./DistributionTerms";
import { ShapePanel } from "./Shape";
import { RETURN_SCALE } from "./distribution";
import {
  type DistributionDraft,
  draftOf,
  problemWith,
  sameSpec,
  specOf,
} from "./distributionDraft";

/** The `<option>` value standing in for "no class"; `null` is not a value. */
const UNCLASSIFIED = "";

/** What the drawer is editing: everything a PATCH to the profile can carry. */
interface ProfileDraft {
  description: string;
  /** Null is the ordinary state: nothing auto-selects an unclassified profile. */
  assetClass: AssetClass | null;
  dist: DistributionDraft;
}

/**
 * The profile half of the drawer — the one place a distribution is edited.
 *
 * Changing Distribution swaps only the terms block; the name, the description
 * and what uses the profile stay put, and the numbers a narrower kind does not
 * take are held until Apply rather than dropped, so a mis-click on the select
 * costs nothing. The shape redraws as the terms are typed, which is the point
 * of having it here at all: the figures say 9.9% and 19.6%, and the curve says
 * what that means for a bad year.
 *
 * Mount with a `key` of the profile so switching rows starts a fresh draft.
 */
export function ProfileInspector({
  profile,
  presets,
  onApply,
  onDuplicate,
  busy,
  error,
  offline,
}: {
  profile: ReturnProfile;
  /** The histories the server offers, for a Bootstrap profile's preset. */
  presets: HistoryPreset[];
  onApply: (body: UpdateProfile) => void;
  onDuplicate: () => void;
  busy?: boolean;
  error?: string;
  /** No connection: the fields close rather than take edits that cannot save. */
  offline?: boolean;
}) {
  const pristine: ProfileDraft = {
    description: profile.description,
    assetClass: profile.assetClass,
    dist: draftOf(profile.distribution, presets),
  };
  const [draft, setDraft] = useState<ProfileDraft>(pristine);
  const spec = specOf(draft.dist);
  // The draft's history, not the stored profile's: switching preset redraws
  // the shape before Apply, which is the only way to compare two of them.
  const history = presets.find((p) => p.id === draft.dist.preset)?.returns;
  const dirty =
    draft.description !== pristine.description ||
    draft.assetClass !== pristine.assetClass ||
    !sameSpec(draft.dist, pristine.dist);
  const problem = problemWith(draft.dist);

  return (
    <div
      style={{
        padding: "16px 18px 20px",
        display: "flex",
        flexDirection: "column",
        gap: 12,
      }}
    >
      <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between" }}>
        <h5 style={{ margin: 0 }}>{profile.id}</h5>
        <Tag tone="outline">profile</Tag>
      </div>

      <Field label="Description">
        <CompactInput
          value={draft.description}
          readOnly={offline}
          placeholder="What this profile stands for"
          onChange={(e) => setDraft((d) => ({ ...d, description: e.target.value }))}
        />
      </Field>

      <Field label="Asset class">
        <Select
          style={{ minHeight: 32 }}
          value={draft.assetClass ?? UNCLASSIFIED}
          disabled={offline}
          onChange={(e) =>
            setDraft((d) => ({
              ...d,
              assetClass:
                e.target.value === UNCLASSIFIED ? null : (e.target.value as AssetClass),
            }))
          }
        >
          <option value={UNCLASSIFIED}>Unclassified — never auto-selected</option>
          {ASSET_CLASSES.map((c) => (
            <option key={c} value={c}>
              {CLASS_LABEL[c]}
            </option>
          ))}
        </Select>
      </Field>
      <p
        style={{
          margin: "-6px 0 0",
          fontSize: 11.5,
          lineHeight: 1.5,
          color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
        }}
      >
        What this assumption is <em>for</em>, as opposed to what it is called. A
        new asset whose ticker is a known fund of this class is mapped here
        automatically — and stays mapped if you rename the profile.
      </p>

      <Field label="Distribution">
        <Select
          style={{ minHeight: 32 }}
          value={draft.dist.kind}
          disabled={offline}
          onChange={(e) =>
            setDraft((d) => ({
              ...d,
              dist: { ...d.dist, kind: e.target.value as DistributionKind },
            }))
          }
        >
          {DISTRIBUTIONS.map((kind) => (
            <option key={kind} value={kind}>
              {KIND_LABEL[kind]}
            </option>
          ))}
        </Select>
      </Field>

      <DistributionTerms
        draft={draft.dist}
        presets={presets}
        readOnly={offline}
        onChange={(patch) => setDraft((d) => ({ ...d, dist: { ...d.dist, ...patch } }))}
      />

      <div>
        <SectionHeading className="mb-[6px]">Shape · annual return</SectionHeading>
        <ShapePanel spec={spec} scale={RETURN_SCALE} history={history} />
      </div>

      <Hr flush />

      <div>
        <SectionHeading className="mb-[6px]">Used by</SectionHeading>
        <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
          {profile.usedBy.length === 0 ? (
            <span className="text-muted" style={{ fontSize: 12 }}>
              Nothing points at this profile yet — an asset or an account&rsquo;s cash
              can.
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

      {(problem ?? error) && (
        <p style={{ margin: 0, fontSize: 12, color: "var(--color-accent-700)" }}>
          {problem ?? error}
        </p>
      )}

      <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
        {/* Revert exists only while there is something to revert; an always-on
            copy of it would read as a third thing the drawer does. */}
        {dirty && (
          <Button style={{ flex: 1 }} disabled={busy} onClick={() => setDraft(pristine)}>
            Revert
          </Button>
        )}
        <Button
          style={{ flex: 1 }}
          onClick={onDuplicate}
          disabled={busy || offline}
          title="Copy this profile, so a shared one can be changed for one scenario"
        >
          Duplicate
        </Button>
        <Button
          variant="primary"
          style={{ flex: 1 }}
          disabled={!dirty || busy || offline || problem != null}
          title={offline ? "No connection to the server." : undefined}
          onClick={() =>
            onApply({
              description: draft.description.trim(),
              // Always sent, so clearing the class is sayable: null here means
              // unclassify, where absent would have meant "leave it alone".
              asset_class: draft.assetClass,
              distribution: spec,
            })
          }
        >
          Apply
        </Button>
      </div>
    </div>
  );
}
