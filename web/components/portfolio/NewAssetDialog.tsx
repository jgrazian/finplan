"use client";

import { useState } from "react";
import { CurrencyInput, Dialog, DialogRow, Field, Input, Select } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { Profile } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";

/** The `<option>` standing in for "no profile"; `null` is not a value. */
const UNMAPPED = -1;

/**
 * Creates an asset: a price series an investment account can hold lots of, or
 * a property account can be valued by. The return profile is what makes it
 * move — and may be left off, in which case the asset holds its opening price
 * for the whole simulation until it is mapped.
 */
export function NewAssetDialog({
  scenarioId,
  profiles,
  onClose,
  onCreated,
}: {
  scenarioId: number;
  profiles: Profile[];
  onClose: () => void;
  onCreated: () => void;
}) {
  const [name, setName] = useState("");
  const [price, setPrice] = useState(100);
  const [profileId, setProfileId] = useState<number | null>(profiles[0]?.id ?? null);
  const submit = useSubmit();

  return (
    <Dialog
      title="New asset"
      onClose={onClose}
      onSubmit={() =>
        submit.run(
          () =>
            api.assets.create(scenarioId, {
              name,
              initial_price: price,
              return_profile_id: profileId,
              sort_order: 0,
            }),
          () => {
            onCreated();
            onClose();
          },
        )
      }
      submitLabel="Create asset"
      busy={submit.busy}
      error={submit.error}
    >
      <DialogRow>
        <Field label="Name">
          <Input value={name} onChange={(e) => setName(e.target.value)} required />
        </Field>
        <Field label="Opening price">
          <CurrencyInput value={price} onValueChange={setPrice} aria-label="Opening price" />
        </Field>
      </DialogRow>
      <Field label="Return profile">
        <Select
          value={profileId ?? UNMAPPED}
          onChange={(e) => {
            const id = Number(e.target.value);
            setProfileId(id === UNMAPPED ? null : id);
          }}
        >
          {profiles.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
          <option value={UNMAPPED}>Unmapped — held flat at 0%</option>
        </Select>
      </Field>
    </Dialog>
  );
}
