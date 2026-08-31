"use client";

import { useState } from "react";
import { pct, quantiles } from "@/components/profiles";
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
import type { Profile } from "@/lib/api/types";
import { fmtCurrency, fmtUnits } from "@/lib/format";
import type { ReturnProfile } from "@/lib/types";
import type { AssetRow } from "@/lib/view/assets";

/** The fields a PATCH can carry; `profileServerId` is what a remap changes. */
export interface AssetDraft {
  ticker: string;
  name: string;
  price: number;
  profileServerId: number;
}

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
 * The asset half of the polymorphic inspector.
 *
 * The figures under "inherited from" are the profile's, not the asset's: an
 * asset has no return of its own, which is the whole reason the list nests it
 * under one. Mount with a `key` of the asset id so switching rows starts a
 * fresh draft rather than carrying edits across.
 */
export function AssetInspector({
  asset,
  profile,
  profiles,
  onApply,
  busy,
  error,
}: {
  asset: AssetRow;
  /** The profile the asset currently points at, for the inherited figures. */
  profile: ReturnProfile | undefined;
  /** Every profile the asset could be remapped onto. */
  profiles: Profile[];
  onApply: (draft: AssetDraft) => void;
  busy?: boolean;
  error?: string;
}) {
  const [draft, setDraft] = useState<AssetDraft>(() => draftOf(asset));
  const set = <K extends keyof AssetDraft>(key: K, value: AssetDraft[K]) =>
    setDraft((d) => ({ ...d, [key]: value }));

  const pristine = draftOf(asset);
  const dirty = (Object.keys(pristine) as Array<keyof AssetDraft>).some(
    (k) => draft[k] !== pristine[k],
  );
  const q = quantiles(profile?.mean ?? null, profile?.sd ?? null);

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
          {asset.ticker}
        </h5>
        <Tag tone="outline">asset</Tag>
      </div>

      <Field label="Name">
        <CompactInput value={draft.name} onChange={(e) => set("name", e.target.value)} />
      </Field>

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
        <Field label="Ticker">
          <CompactInput value={draft.ticker} onChange={(e) => set("ticker", e.target.value)} />
        </Field>
        <Field label="Opening price">
          <CurrencyInput
            style={{ minHeight: 32 }}
            value={draft.price}
            onValueChange={(price) => set("price", price)}
            aria-label="Opening price"
          />
        </Field>
      </div>

      <Field label="Return profile">
        <Select
          style={{ minHeight: 32 }}
          value={draft.profileServerId}
          onChange={(e) => set("profileServerId", Number(e.target.value))}
        >
          {profiles.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </Select>
      </Field>

      <Blueprint style={{ padding: "9px 11px" }}>
        <StatLabel>inherited from {profile?.id ?? "—"}</StatLabel>
        <div style={{ display: "flex", gap: 18, marginTop: 4 }}>
          <div>
            <StatLabel>mean</StatLabel>
            <div style={FIGURE}>{pct(profile?.mean ?? null)}</div>
          </div>
          <div>
            <StatLabel>vol</StatLabel>
            <div style={FIGURE}>{profile?.sd === 0 ? "—" : pct(profile?.sd ?? null)}</div>
          </div>
          <div>
            <StatLabel>5th–95th</StatLabel>
            <div style={FIGURE}>
              {q.p5} … {q.p95}
            </div>
          </div>
        </div>
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
            <div style={{ fontSize: 12.5, color: "color-mix(in srgb, var(--color-text) 62%, transparent)" }}>
              {asset.holdings.length} account{asset.holdings.length === 1 ? "" : "s"} ·{" "}
              {fmtUnits(asset.units)} sh · basis {fmtCurrency(asset.costBasis)}
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
          disabled={!dirty || busy}
          onClick={() => onApply(draft)}
        >
          Apply
        </Button>
      </div>
    </div>
  );
}
