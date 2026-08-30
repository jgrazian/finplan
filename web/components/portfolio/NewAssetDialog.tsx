"use client";

import { useState } from "react";
import { Dialog, DialogRow, Field, Input, Select } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { Profile } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";

/**
 * Creates an asset: a price series an investment account can hold lots of, or
 * a property account can be valued by. The return profile is what makes it
 * move.
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
  const [price, setPrice] = useState("100");
  const [profileId, setProfileId] = useState(profiles[0]?.id ?? 0);
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
              initial_price: Number(price) || 0,
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
          <Input type="number" step="any" value={price} onChange={(e) => setPrice(e.target.value)} />
        </Field>
      </DialogRow>
      <Field label="Return profile">
        <Select value={profileId} onChange={(e) => setProfileId(Number(e.target.value))}>
          {profiles.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </Select>
      </Field>
    </Dialog>
  );
}
