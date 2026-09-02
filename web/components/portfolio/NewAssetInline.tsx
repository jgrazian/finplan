"use client";

import { useState } from "react";
import { Blueprint, Button, CompactInput, CurrencyInput, Field } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { Asset } from "@/lib/api/types";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";

/**
 * Create an asset without leaving the sentence that needed one.
 *
 * Both places that ask you to pick an asset — a property account's underlying
 * asset, and the ticker a lot is held in — can hit a scenario that has none, or
 * none that fit. Sending you to Assets & returns to make one loses the account
 * or lot you were halfway through describing, so the asset is made here.
 *
 * It is deliberately two fields. The return profile, which is the only other
 * thing an asset has, is left unset: an unmapped asset compiles at flat zero
 * growth rather than refusing to run, so the plan stays valid and the mapping
 * is a later, deliberate decision on the screen built for it.
 *
 * Renders as a block rather than a dialog because one of its callers is already
 * inside a dialog and the other is inside a drawer; a second modal over either
 * would cover the thing being described. Every button is `type="button"` so it
 * cannot submit the form it may be sitting in.
 */
export function NewAssetInline({
  scenarioId,
  suggestedName,
  onCreated,
  onCancel,
}: {
  scenarioId: number;
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

  const create = () => {
    const ticker = name.trim();
    if (ticker === "") return setError("An asset needs a ticker.");
    if (!Number.isFinite(price) || price <= 0) {
      return setError("Opening price must be positive.");
    }
    setBusy(true);
    setError(undefined);
    api.assets
      .create(scenarioId, { name: ticker, initial_price: price, sort_order: 0 })
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
          Created unmapped: it holds this price for the whole simulation until
          you give it a return profile on Assets &amp; returns.
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
