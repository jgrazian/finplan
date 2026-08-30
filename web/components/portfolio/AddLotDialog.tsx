"use client";

import { useState } from "react";
import { Dialog, DialogRow, Field, Input, Select } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { Asset } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";

/**
 * Adds a purchase lot to an investment account.
 *
 * Cost basis is per lot rather than per asset because the engine's liquidation
 * strategies (FIFO, highest-cost, …) pick between lots, and the gain each one
 * realises depends on what it was bought for.
 */
export function AddLotDialog({
  scenarioId,
  accountId,
  assets,
  onClose,
  onCreated,
}: {
  scenarioId: number;
  accountId: number;
  assets: Asset[];
  onClose: () => void;
  onCreated: () => void;
}) {
  const [assetId, setAssetId] = useState(assets[0]?.id ?? 0);
  const [units, setUnits] = useState("0");
  const [basis, setBasis] = useState("0");
  const [date, setDate] = useState("");
  const submit = useSubmit();

  return (
    <Dialog
      title="Add lot"
      onClose={onClose}
      onSubmit={() =>
        submit.run(
          () =>
            api.accounts.addPosition(scenarioId, accountId, {
              asset_id: assetId,
              units: Number(units) || 0,
              cost_basis: Number(basis) || 0,
              // Omitted means the scenario's start date: an opening holding.
              purchase_date: date.trim() === "" ? null : date,
            }),
          () => {
            onCreated();
            onClose();
          },
        )
      }
      submitLabel="Add lot"
      busy={submit.busy}
      error={
        assets.length === 0 ? "This scenario has no assets to hold yet." : submit.error
      }
    >
      <DialogRow>
        <Field label="Asset">
          <Select value={assetId} onChange={(e) => setAssetId(Number(e.target.value))}>
            {assets.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="Units">
          <Input type="number" step="any" value={units} onChange={(e) => setUnits(e.target.value)} />
        </Field>
      </DialogRow>
      <DialogRow>
        <Field label="Cost basis (total paid)">
          <Input type="number" step="any" value={basis} onChange={(e) => setBasis(e.target.value)} />
        </Field>
        <Field label="Purchase date">
          <Input
            type="date"
            value={date}
            placeholder="plan start"
            onChange={(e) => setDate(e.target.value)}
          />
        </Field>
      </DialogRow>
    </Dialog>
  );
}
