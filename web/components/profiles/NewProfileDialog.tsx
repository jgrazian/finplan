"use client";

import { useState } from "react";
import { Dialog, Dropdown, Field, Input } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { AssetClass, HistoryPreset, Profile } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { ASSET_CLASSES, CLASS_LABEL } from "@/lib/tickers";
import type { DistributionKind } from "@/lib/types";
import { DISTRIBUTIONS, DistributionTerms, KIND_LABEL } from "./DistributionTerms";
import { ShapePanel } from "./Shape";
import { RETURN_SCALE } from "./distribution";
import { draftOf, problemWith, specOf } from "./distributionDraft";

/** The dropdown value standing in for "no class". */
const UNCLASSIFIED = "";

/**
 * Creates a return profile — a market assumption assets and accounts can share.
 *
 * The library is per user rather than per scenario, so a profile made here is
 * one every scenario can point at. Which is why the shape is drawn as it is
 * typed: it is easier to see that 20% volatility means a bad year of −25% than
 * to work it back out of the number afterwards, in a scenario that used it.
 */
export function NewProfileDialog({
  presets,
  onClose,
  onCreated,
}: {
  presets: HistoryPreset[];
  onClose: () => void;
  onCreated: (profile: Profile) => void;
}) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [assetClass, setAssetClass] = useState<AssetClass | null>(null);
  const [dist, setDist] = useState(() => draftOf({ kind: "Normal", mean: 0.07, std_dev: 0.15 }, presets));
  const submit = useSubmit();
  const problem = problemWith(dist);
  const history = presets.find((p) => p.id === dist.preset)?.returns;

  return (
    <Dialog
      title="New return profile"
      onClose={onClose}
      onSubmit={() => {
        if (problem) return submit.fail(problem);
        submit.run(
          () =>
            api.returnProfiles
              .create({
                name: name.trim(),
                description: description.trim() || null,
                asset_class: assetClass,
                distribution: specOf(dist),
              })
              .then(onCreated),
          onClose,
        );
      }}
      submitLabel="Create profile"
      busy={submit.busy}
      error={submit.error}
    >
      <Field label="Name">
        <Input value={name} onChange={(e) => setName(e.target.value)} required />
      </Field>
      <Field label="Description">
        <Input
          value={description}
          placeholder="What this profile stands for"
          onChange={(e) => setDescription(e.target.value)}
        />
      </Field>
      <Field label="Asset class">
        <Dropdown<AssetClass | typeof UNCLASSIFIED>
          className="dd-field"
          ariaLabel="Asset class"
          value={assetClass ?? UNCLASSIFIED}
          onChange={(value) => setAssetClass(value === UNCLASSIFIED ? null : value)}
          options={[
            // Hand-made profiles default to unclassified so they do not compete
            // with the library's existing class when matching a ticker.
            { value: UNCLASSIFIED, label: "Unclassified — never auto-selected" },
            ...ASSET_CLASSES.map((value) => ({ value, label: CLASS_LABEL[value] })),
          ]}
        />
      </Field>
      <Field label="Distribution">
        <Dropdown<DistributionKind>
          className="dd-field"
          ariaLabel="Distribution"
          value={dist.kind}
          onChange={(kind) => setDist((d) => ({ ...d, kind }))}
          options={DISTRIBUTIONS.map((value) => ({ value, label: KIND_LABEL[value] }))}
        />
      </Field>

      <DistributionTerms
        draft={dist}
        presets={presets}
        onChange={(patch) => setDist((d) => ({ ...d, ...patch }))}
      />

      <ShapePanel spec={specOf(dist)} scale={RETURN_SCALE} height={64} history={history} />
    </Dialog>
  );
}
