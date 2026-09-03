"use client";

import { useState } from "react";
import { Blueprint, Button, CompactInput, CurrencyInput, Field } from "@/components/ui";
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
 * It stays two fields. An asset's other two — its name and the profile that
 * makes it move — are read off the ticker where the ticker is one the bundled
 * table knows, which is most of what a plan is built out of. What was inferred
 * is shown under the field before anything is created, and an unrecognised
 * ticker is created exactly as it always was: named only by its symbol, and
 * unmapped, which compiles at flat zero growth rather than refusing to run.
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

  const known = tickerDefaults(name, profiles);

  const create = () => {
    const ticker = name.trim();
    if (ticker === "") return setError("An asset needs a ticker.");
    if (!Number.isFinite(price) || price <= 0) {
      return setError("Opening price must be positive.");
    }
    setBusy(true);
    setError(undefined);
    api.assets
      .create(scenarioId, {
        name: ticker,
        description: known?.name ?? null,
        initial_price: price,
        return_profile_id: known?.profile?.id ?? null,
        sort_order: 0,
      })
      .then(onCreated)
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)))
      .finally(() => setBusy(false));
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
            onCancel();
          }
        }}
      >
        <h6 style={{ margin: "0 0 9px" }}>New asset</h6>

        <div style={{ display: "grid", gridTemplateColumns: "1fr 118px", gap: 8 }}>
          <Field label="Ticker">
            <CompactInput
              autoFocus
              value={name}
              placeholder="VTI"
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          <Field label="Opening price">
            <CurrencyInput
              style={{ minHeight: 32 }}
              value={price}
              onValueChange={setPrice}
              aria-label="Opening price"
            />
          </Field>
        </div>

        <p style={{ margin: "8px 0 0", fontSize: 11.5, lineHeight: 1.5, color: MUTED }}>
          {known == null ? (
            <>
              Created unmapped: it holds this price for the whole simulation
              until you give it a return profile on Assets &amp; returns.
            </>
          ) : (
            <>
              <strong style={{ fontWeight: 500, color: "var(--color-text)" }}>
                {known.name}
              </strong>{" "}
              · {known.classLabel} ·{" "}
              {known.profile
                ? `grows by ${known.profile.name}`
                : "no matching profile — held flat at 0%"}
            </>
          )}
        </p>

        {error && (
          <p style={{ margin: "8px 0 0", fontSize: 12, color: "var(--color-accent-700)" }}>
            {error}
          </p>
        )}

        <div style={{ display: "flex", gap: 8, marginTop: 11, justifyContent: "flex-end" }}>
          <Button onClick={onCancel}>Cancel</Button>
          <Button variant="primary" disabled={busy} onClick={create}>
            {busy ? "…" : "Create asset"}
          </Button>
        </div>
      </div>
    </Blueprint>
  );
}
