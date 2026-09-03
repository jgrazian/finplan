"use client";

import { useState } from "react";
import { RETURN_SCALE, ShapePanel, pct } from "@/components/profiles";
import {
  Blueprint,
  Button,
  CompactInput,
  CurrencyInput,
  Field,
  Hr,
  SectionHeading,
  Select,
  StatLabel,
  Table,
  Tag,
  Td,
} from "@/components/ui";
import type { DistributionSpec, Profile } from "@/lib/api/types";
import { fmtCurrency, fmtUnits } from "@/lib/format";
import type { ReturnProfile } from "@/lib/types";
import type { AssetRow } from "@/lib/view/assets";

/** The fields a PATCH can carry; `profileServerId` is what a remap changes. */
export interface AssetDraft {
  ticker: string;
  name: string;
  price: number;
  /** Null leaves the asset unmapped, which the engine holds flat at 0%. */
  profileServerId: number | null;
}

/** The `<option>` value standing in for "no profile"; `null` is not a value. */
const UNMAPPED = -1;

/** An unmapped asset is held flat, which is what a None profile draws. */
const HELD_FLAT: DistributionSpec = { kind: "None" };

const FIGURE = {
  fontFamily: "var(--font-heading)",
  fontWeight: 600,
  fontSize: 16,
} as const;

function draftOf(asset: AssetRow): AssetDraft {
  return {
    ticker: asset.ticker,
    name: asset.name,
    price: asset.price,
    profileServerId: asset.profileServerId,
  };
}

/**
 * The asset half of the drawer.
 *
 * The block under the return profile is the profile's, not the asset's, and it
 * is read-only here: an asset has no return of its own, which is the whole
 * reason it points at one. Editing the profile is a different subject, so it is
 * a move to the library rather than a second set of fields for the same row.
 *
 * Mount with a `key` of the asset id so switching rows starts a fresh draft
 * rather than carrying edits across.
 */
export function AssetInspector({
  asset,
  profile,
  profiles,
  onApply,
  onEditProfile,
  busy,
  error,
  offline,
}: {
  asset: AssetRow;
  /** The profile the asset currently points at, for the inherited figures. */
  profile: ReturnProfile | undefined;
  /** Every profile the asset could be remapped onto. */
  profiles: Profile[];
  onApply: (draft: AssetDraft) => void;
  /** Moves the drawer onto the profile itself; absent while unmapped. */
  onEditProfile?: () => void;
  busy?: boolean;
  error?: string;
  /** No connection: the fields close rather than take edits that cannot save. */
  offline?: boolean;
}) {
  const [draft, setDraft] = useState<AssetDraft>(() => draftOf(asset));
  const set = <K extends keyof AssetDraft>(key: K, value: AssetDraft[K]) =>
    setDraft((d) => ({ ...d, [key]: value }));

  const pristine = draftOf(asset);
  const dirty = (Object.keys(pristine) as Array<keyof AssetDraft>).some(
    (k) => draft[k] !== pristine[k],
  );
  // The block describes what the asset points at now, not what the unsaved
  // draft would point at: the figures are the stored profile's until Apply.
  const spec = profile?.distribution ?? HELD_FLAT;

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
        <h5 style={{ margin: 0, fontFamily: "ui-monospace, Menlo, monospace", fontSize: 15 }}>
          {draft.ticker || "—"}
        </h5>
        <Tag tone="outline">asset</Tag>
      </div>

      <Field label="Name">
        <CompactInput
          value={draft.name}
          readOnly={offline}
          onChange={(e) => set("name", e.target.value)}
        />
      </Field>

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
        <Field label="Ticker">
          <CompactInput
            value={draft.ticker}
            readOnly={offline}
            onChange={(e) => set("ticker", e.target.value)}
          />
        </Field>
        <Field label="Opening price">
          <CurrencyInput
            style={{ minHeight: 32 }}
            value={draft.price}
            readOnly={offline}
            onValueChange={(price) => set("price", price)}
            aria-label="Opening price"
          />
        </Field>
      </div>

      <Field label="Return profile">
        <Select
          style={{ minHeight: 32 }}
          value={draft.profileServerId ?? UNMAPPED}
          disabled={offline}
          onChange={(e) => {
            const id = Number(e.target.value);
            set("profileServerId", id === UNMAPPED ? null : id);
          }}
        >
          <option value={UNMAPPED}>Unmapped — held flat at 0%</option>
          {profiles.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </Select>
      </Field>

      <Blueprint style={{ padding: "9px 11px" }}>
        {profile == null ? (
          <>
            <StatLabel>no profile</StatLabel>
            <p style={{ margin: "5px 0 0", fontSize: 12, lineHeight: 1.5 }}>
              A run still compiles: the asset holds its opening price for the
              whole simulation. Anything held in it is therefore flat in real
              terms, which is almost never what you mean for long.
            </p>
          </>
        ) : (
          <>
            <div
              style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between" }}
            >
              <StatLabel>inherited · {profile.kind}</StatLabel>
              {onEditProfile && (
                <button type="button" className="linkbtn" style={{ fontSize: 11 }} onClick={onEditProfile}>
                  Edit profile ›
                </button>
              )}
            </div>
            {/* Mean and spread only: the shape below carries the 5th, the
                median and the 95th, written under the marks that place them. */}
            <div style={{ display: "flex", gap: 18, margin: "4px 0 6px" }}>
              <div>
                <StatLabel>mean</StatLabel>
                <div style={FIGURE}>{pct(profile.mean)}</div>
              </div>
              <div>
                <StatLabel>vol</StatLabel>
                <div style={FIGURE}>{profile.sd === 0 ? "—" : pct(profile.sd)}</div>
              </div>
            </div>
            <ShapePanel
              spec={spec}
              scale={RETURN_SCALE}
              height={64}
              footer={false}
              frame={false}
              history={profile.history}
            />
          </>
        )}
      </Blueprint>

      <Hr flush />

      <div>
        <SectionHeading className="mb-[6px]">Held in</SectionHeading>
        {asset.holdings.length === 0 ? (
          <span className="text-muted" style={{ fontSize: 12 }}>
            No account holds this asset yet — add a lot from Accounts, or point a
            property account at it.
          </span>
        ) : (
          <>
            <div
              style={{
                fontSize: 12.5,
                color: "color-mix(in srgb, var(--color-text) 62%, transparent)",
              }}
            >
              {asset.heldIn} · {fmtUnits(asset.units)} sh · {fmtCurrency(asset.value)} · basis{" "}
              {fmtCurrency(asset.costBasis)}
            </div>
            <Table compact className="mt-[6px]">
              <tbody>
                {asset.holdings.map((h) => (
                  <tr key={h.account}>
                    <Td>{h.account}</Td>
                    <Td align="right">{fmtUnits(h.units)}</Td>
                    <Td align="right">{fmtCurrency(h.costBasis)}</Td>
                  </tr>
                ))}
              </tbody>
            </Table>
          </>
        )}
      </div>

      {error && (
        <p style={{ margin: 0, fontSize: 12, color: "var(--color-accent-700)" }}>{error}</p>
      )}

      <div style={{ display: "flex", gap: 8, marginTop: "auto" }}>
        <Button
          style={{ flex: 1 }}
          disabled={!dirty || busy}
          onClick={() => setDraft(draftOf(asset))}
        >
          Revert
        </Button>
        <Button
          variant="primary"
          style={{ flex: 1 }}
          disabled={!dirty || busy || offline}
          title={offline ? "No connection to the server." : undefined}
          onClick={() => onApply(draft)}
        >
          Apply
        </Button>
      </div>
    </div>
  );
}
