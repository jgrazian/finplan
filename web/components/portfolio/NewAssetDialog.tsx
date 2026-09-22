"use client";

import { useState } from "react";
import { CurrencyInput, Dialog, DialogRow, Dropdown, Field, Input } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { Profile } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { tickerDefaults } from "@/lib/tickers";
import { UNMAPPED, profileOptions } from "./profilePicker";

/**
 * Creates an asset: a price series an investment account can hold lots of, or
 * a property account can be valued by. The return profile is what makes it
 * move — and may be left off, in which case the asset holds its opening price
 * for the whole simulation until it is mapped.
 *
 * Name and profile follow the ticker while nobody has said otherwise: `VTI` is
 * the total US market, and a total-market fund belongs on the library's US
 * equity profile. Typing into either field takes it off the ticker for good, so
 * a later correction to the symbol cannot overwrite what was chosen by hand.
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
  const [ticker, setTicker] = useState("");
  const [price, setPrice] = useState(100);
  /** Null while the name is still the ticker's; a string once it is the user's. */
  const [name, setName] = useState<string | null>(null);
  /** Undefined while the mapping is still the ticker's — `null` means unmapped. */
  const [profileId, setProfileId] = useState<number | null | undefined>(undefined);
  const submit = useSubmit();

  const known = tickerDefaults(ticker, profiles);
  const effectiveName = name ?? known?.name ?? "";
  const effectiveProfile =
    profileId === undefined
      ? (known?.profile?.id ?? profiles[0]?.id ?? null)
      : profileId;

  return (
    <Dialog
      title="New asset"
      onClose={onClose}
      onSubmit={() =>
        submit.run(
          () =>
            api.assets.create(scenarioId, {
              name: ticker,
              description: effectiveName.trim() === "" ? null : effectiveName.trim(),
              initial_price: price,
              return_profile_id: effectiveProfile,
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
        <Field label="Ticker">
          <Input
            value={ticker}
            placeholder="VTI"
            onChange={(e) => setTicker(e.target.value)}
            required
          />
        </Field>
        <Field label="Opening price">
          <CurrencyInput value={price} onValueChange={setPrice} aria-label="Opening price" />
        </Field>
      </DialogRow>
      <Field label="Name">
        <Input
          value={effectiveName}
          placeholder={known ? known.name : "optional"}
          onChange={(e) => setName(e.target.value)}
        />
      </Field>
      <Field label="Return profile">
        <Dropdown
          className="dd-field"
          options={profileOptions(profiles)}
          value={effectiveProfile ?? UNMAPPED}
          maxMenuHeight={300}
          ariaLabel="Return profile"
          onChange={(id) => setProfileId(id === UNMAPPED ? null : id)}
        />
      </Field>
      {known && (
        <p
          style={{
            margin: 0,
            fontSize: 11.5,
            lineHeight: 1.5,
            color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
          }}
        >
          Recognised as {known.classLabel}
          {known.profile
            ? `, mapped to ${known.profile.name}.`
            : ", and nothing in your library describes it — leave it unmapped or pick a profile."}
        </p>
      )}
    </Dialog>
  );
}
