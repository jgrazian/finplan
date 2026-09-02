"use client";

import { useState } from "react";
import {
  Blueprint,
  Button,
  CurrencyInput,
  DateInput,
  Dropdown,
  Field,
  NumberInput,
  SegmentedControl,
  type SegmentOption,
} from "@/components/ui";
import type { Asset, CreatePosition, Profile } from "@/lib/api/types";
import { fmtCurrency, fmtUnits } from "@/lib/format";
import type { TaxStatus } from "@/lib/types";

type Mode = "value" | "units";

const MODES: ReadonlyArray<SegmentOption<Mode>> = [
  { value: "value", label: "Value" },
  { value: "units", label: "Units" },
];

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";

const FIGURE = {
  // The floor the ".dd-field" trigger beside it also takes; above it both are
  // sized by the same line box, so the pair stays level.
  minHeight: 36,
  fontSize: 15,
  fontFamily: "var(--font-heading)",
  fontWeight: 600,
} as const;

const DAY_MS = 86_400_000;

/** A holding is long-term once it has been held a year, which is what the rate turns on. */
function heldLongTerm(iso: string): boolean {
  const bought = Date.parse(iso);
  return Number.isFinite(bought) && Date.now() - bought >= 365 * DAY_MS;
}

/**
 * Add a position — the amount first, the lot detail opt-in.
 *
 * Four required fields become two. Amount and ticker are one sentence; units
 * and value are the same field under a switch, converted at the asset's opening
 * price. Cost basis and purchase date stay available but are demoted to a
 * checkbox, and in a tax-deferred or tax-free account they are not offered at
 * all — nothing in the simulation reads them there.
 *
 * It opens under the positions table inside the drawer rather than over it, so
 * the account stays on screen and repeat entry is one Enter away.
 */
export function AddPositionForm({
  assets,
  profiles,
  taxStatus,
  busy,
  error,
  onCancel,
  onSubmit,
}: {
  assets: Asset[];
  profiles: Profile[];
  /** Basis is only offered where a sale could realise a gain. */
  taxStatus: TaxStatus | undefined;
  busy?: boolean;
  error?: string;
  onCancel: () => void;
  /** `again` keeps the form open for the next lot. */
  onSubmit: (body: CreatePosition, again: boolean) => void;
}) {
  const [mode, setMode] = useState<Mode>("value");
  const [assetId, setAssetId] = useState(assets[0]?.id);
  const [amount, setAmount] = useState(0);
  const [tracking, setTracking] = useState(false);
  const [basis, setBasis] = useState(0);
  const [date, setDate] = useState("");

  const asset = assets.find((a) => a.id === assetId);
  const price = asset?.initial_price ?? 0;
  const units = mode === "units" ? amount : price > 0 ? amount / price : 0;
  const value = mode === "units" ? amount * price : amount;
  const profile = profiles.find((p) => p.id === asset?.return_profile_id);

  // Basis is per lot because the engine's liquidation strategies pick between
  // lots, and the gain each one realises depends on what it was bought for.
  // Where gains are never realised, the field is noise.
  const offersBasis = taxStatus === "Taxable";
  const tracked = offersBasis && tracking;

  const submit = (again: boolean) => {
    if (assetId == null || units <= 0 || busy) return;
    onSubmit(
      {
        asset_id: assetId,
        units,
        // Untracked, the lot is marked at today's value — a basis equal to what
        // it is worth realises no gain, which is exactly what "not tracked"
        // has to mean to the engine.
        cost_basis: tracked ? basis : value,
        // Omitted means the scenario's start date: an opening holding.
        purchase_date: tracked && date.trim() !== "" ? date : null,
      },
      again,
    );
  };

  const gain = value - basis;

  return (
    <Blueprint
      style={{
        marginTop: 12,
        padding: "12px 13px 14px",
        background: "color-mix(in srgb, var(--color-accent) 6%, transparent)",
      }}
    >
      <div
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            // The drawer binds the same chord to Apply; this one is the form's.
            e.stopPropagation();
            submit(false);
          }
          if (e.key === "Escape") {
            e.preventDefault();
            // The drawer also listens for Escape to revert its draft; this one
            // is aimed at the form.
            e.stopPropagation();
            onCancel();
          }
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            marginBottom: 10,
          }}
        >
          <h6 style={{ margin: 0 }}>Add position</h6>
          <SegmentedControl
            options={MODES}
            value={mode}
            onChange={(next) => {
              // The switch changes the unit the same holding is written in, not
              // the holding — so carry the figure across rather than reset it.
              setAmount(next === "units" ? units : value);
              setMode(next);
            }}
            ariaLabel="Enter the amount as a value or as units"
          />
        </div>

        <div
          style={{
            display: "grid",
            gridTemplateColumns: "1fr 118px",
            gap: 8,
            alignItems: "end",
          }}
        >
          <Field label="I hold">
            {mode === "value" ? (
              <CurrencyInput
                style={FIGURE}
                value={amount}
                onValueChange={setAmount}
                aria-label="Amount held, by value"
              />
            ) : (
              <NumberInput
                style={FIGURE}
                value={amount}
                decimals={2}
                suffix="units"
                onValueChange={setAmount}
                aria-label="Amount held, in units"
              />
            )}
          </Field>
          <Field label="of">
            <Dropdown
              className="dd-field dd-figure"
              options={assets.map((a) => ({ value: a.id, label: a.name }))}
              value={assetId ?? null}
              onChange={setAssetId}
              placeholder="asset"
              ariaLabel="Asset held"
            />
          </Field>
        </div>

        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            marginTop: 7,
            fontSize: 11.5,
            color: MUTED,
          }}
        >
          <span
            style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
          >
            {asset ? (asset.description ?? asset.name) : "No assets yet"}
            {profile ? ` · ${profile.name}` : ""}
          </span>
          {asset && (
            <span style={{ marginLeft: "auto", flex: "none" }}>
              &asymp;{" "}
              {mode === "value" ? `${fmtUnits(units)} units` : fmtCurrency(value)} @{" "}
              {fmtCurrency(price)}
            </span>
          )}
        </div>

        {offersBasis && (
          <div
            style={{
              marginTop: 14,
              paddingTop: 12,
              borderTop: "1px solid var(--color-divider)",
            }}
          >
            <label
              className="radio"
              style={{
                display: "flex",
                alignItems: tracking ? "center" : "flex-start",
                gap: 9,
                fontSize: 12.5,
                marginBottom: tracking ? 10 : 0,
              }}
            >
              <input
                type="checkbox"
                checked={tracking}
                onChange={(e) => setTracking(e.target.checked)}
              />
              <span className="dot" style={{ marginTop: tracking ? 0 : 2 }} />
              <span>
                <span>Track cost basis and purchase date</span>
                {!tracking && (
                  <span style={{ display: "block", fontSize: 11, marginTop: 2, color: MUTED }}>
                    Only changes results if a sale in your plan realises capital
                    gains. Left off, the lot is marked at today&rsquo;s value.
                  </span>
                )}
              </span>
            </label>

            {tracking && (
              <>
                <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
                  <Field label="Cost basis">
                    <CurrencyInput
                      style={{ minHeight: 32 }}
                      value={basis}
                      onValueChange={setBasis}
                      aria-label="Cost basis, total paid"
                    />
                  </Field>
                  <Field label="Purchased">
                    <DateInput
                      style={{ minHeight: 32 }}
                      value={date}
                      placeholder="plan start"
                      onValueChange={setDate}
                    />
                  </Field>
                </div>
                <div style={{ fontSize: 11.5, marginTop: 7, color: MUTED }}>
                  {gain >= 0 ? "Unrealised gain" : "Unrealised loss"}{" "}
                  {fmtCurrency(Math.abs(gain))}
                  {date.trim() !== "" && ` · ${heldLongTerm(date) ? "long" : "short"}-term`}
                </div>
              </>
            )}
          </div>
        )}

        {error && (
          <p style={{ margin: "10px 0 0", fontSize: 12, color: "var(--color-accent-900)" }}>
            {error}
          </p>
        )}

        <div
          style={{
            display: "flex",
            gap: 8,
            marginTop: 12,
            alignItems: "center",
            flexWrap: "wrap",
          }}
        >
          <Button
            variant="ghost"
            disabled={busy || units <= 0}
            onClick={() => submit(true)}
          >
            Save &amp; add another
          </Button>
          <Button style={{ marginLeft: "auto" }} onClick={onCancel}>
            Cancel
          </Button>
          <Button
            variant="primary"
            shortcut="⌘⏎"
            disabled={busy || units <= 0}
            onClick={() => submit(false)}
          >
            Add position
          </Button>
        </div>
      </div>
    </Blueprint>
  );
}
