"use client";

import { useRef, useState } from "react";
import { Blueprint, Button, CompactInput, CurrencyInput, Dropdown, Field } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { Asset, Profile } from "@/lib/api/types";
import { tickerDefaults } from "@/lib/tickers";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";

/**
 * Create an asset without leaving the sentence that needed one.
 *
 * Both places that ask you to pick an asset — a property account's underlying
 * asset, and the ticker a lot is held in — can hit a scenario that has none, or
 * none that fit. Sending you to Assets & returns to make one loses the account
 * or lot you were halfway through describing, so the asset is made here.
 *
 * Recognised tickers suggest a return profile. Users can override it here;
 * an unknown ticker requires a choice, including explicit no-growth modeling.
 *
 * Renders as a block rather than a dialog because one of its callers is already
 * inside a dialog and the other is inside a drawer; a second modal over either
 * would cover the thing being described. Every button is `type="button"` so it
 * cannot submit the form it may be sitting in.
 */
export function NewAssetInline({
  scenarioId,
  profiles,
  suggestedName,
  onCreated,
  onCancel,
}: {
  scenarioId: number;
  /** The library a recognised ticker is mapped into. */
  profiles: Profile[];
  /** Prefills the ticker, e.g. from the account being named. */
  suggestedName?: string;
  /** The created row, so the caller can select it before the reload lands. */
  onCreated: (asset: Asset) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(suggestedName ?? "");
  const [price, setPrice] = useState(100);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [profileChoice, setProfileChoice] = useState<number | null | undefined>();
  const submitting = useRef(false);

  const known = tickerDefaults(name, profiles);
  const profileId = profileChoice === undefined ? known?.profile?.id : profileChoice;

  const create = () => {
    if (submitting.current) return;
    const ticker = name.trim();
    if (ticker === "") return setError("An asset needs a ticker.");
    if (!Number.isFinite(price) || price <= 0) {
      return setError("Opening price must be positive.");
    }
    if (profileId === undefined) {
      return setError("Choose a return profile, or explicitly choose no price growth.");
    }
    if (profileId !== null && !profiles.some((profile) => profile.id === profileId)) {
      return setError("That return profile is no longer available. Choose another.");
    }
    submitting.current = true;
    setBusy(true);
    setError(undefined);
    api.assets
      .create(scenarioId, {
        name: ticker,
        description: known?.name ?? null,
        initial_price: price,
        return_profile_id: profileId,
        sort_order: 0,
      })
      .then(onCreated)
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)))
      .finally(() => {
        submitting.current = false;
        setBusy(false);
      });
  };

  return (
    <Blueprint
      corners={false}
      style={{
        padding: "11px 12px 12px",
        background: "color-mix(in srgb, var(--color-accent) 6%, transparent)",
      }}
    >
      <div
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            // The enclosing dialog submits on Enter, and the drawer applies on
            // it; while this block is open, Enter belongs to it.
            e.preventDefault();
            e.stopPropagation();
            create();
          }
          if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            if (!submitting.current) onCancel();
          }
        }}
      >
        <h6 style={{ margin: "0 0 9px" }}>New asset</h6>

        <div style={{ display: "grid", gridTemplateColumns: "1fr 118px", gap: 8 }}>
          <Field label="Ticker">
            <CompactInput
              autoFocus
              value={name}
              aria-label="Ticker"
              disabled={busy}
              placeholder="VTI"
              onChange={(e) => {
                setName(e.target.value);
                setProfileChoice(undefined);
                setError(undefined);
              }}
            />
          </Field>
          <Field label="Opening price">
            <CurrencyInput
              style={{ minHeight: 32 }}
              value={price}
              disabled={busy}
              onValueChange={setPrice}
              aria-label="Opening price"
            />
          </Field>
        </div>

        <Field label="Return profile" style={{ marginTop: 8 }}>
          <Dropdown<number | "flat">
            className="dd-field"
            ariaLabel="New asset return profile"
            value={profileId === null ? "flat" : profileId}
            disabled={busy}
            placeholder="Choose a return assumption"
            onChange={(value) => {
              setProfileChoice(value === "flat" ? null : value);
              setError(undefined);
            }}
            options={[
              ...profiles.map((profile) => ({ value: profile.id, label: profile.name })),
              { value: "flat", label: "No price growth — keep the opening price" },
            ]}
          />
        </Field>

        <p style={{ margin: "8px 0 0", fontSize: 11.5, lineHeight: 1.5, color: MUTED }}>
          {known && `${known.name} · ${known.classLabel}. `}
          {profileId === undefined
            ? "Choose how this asset's price changes in the simulation."
            : profileId === null
              ? "The nominal price stays fixed. Its purchasing power falls when inflation is positive."
              : "The selected profile models returns; it is an assumption, not a price forecast."}
        </p>

        {error && (
          <p role="alert" style={{ margin: "8px 0 0", fontSize: 12, color: "var(--color-accent-700)" }}>
            {error}
          </p>
        )}

        <div style={{ display: "flex", gap: 8, marginTop: 11, justifyContent: "flex-end" }}>
          <Button disabled={busy} onClick={onCancel}>Cancel</Button>
          <Button variant="primary" disabled={busy} onClick={create}>
            {busy ? "…" : "Create asset"}
          </Button>
        </div>
      </div>
    </Blueprint>
  );
}
