"use client";

import { useState } from "react";
import { SubTabBar } from "@/components/layout";
import type { SegmentOption } from "@/components/ui";
import type { RawWorkspace } from "@/lib/hooks/useWorkspace";
import type { Account, InflationProfile, ReturnProfile } from "@/lib/types";
import { AccountsScreen } from "./AccountsScreen";
import { AssetsReturnsScreen } from "./AssetsReturnsScreen";

type PortfolioSection = "accounts" | "returns";

const SECTIONS: ReadonlyArray<SegmentOption<PortfolioSection>> = [
  { value: "accounts", label: "Accounts" },
  { value: "returns", label: "Assets & returns" },
];

const CAPTIONS: Record<PortfolioSection, string | undefined> = {
  accounts: undefined,
  returns:
    "Two tabs became one: a ticker only exists to point at a return profile, so the profile is the group.",
};

/** Portfolio tab: the scenario's accounts, and its assets under the profiles that drive them. */
export function PortfolioScreen({
  scenarioId,
  accounts,
  raw,
  returnProfiles,
  inflationProfiles,
  activeInflationProfile,
  onActivateInflation,
  onChanged,
}: {
  scenarioId: number;
  accounts: Account[];
  raw: RawWorkspace;
  returnProfiles: ReturnProfile[];
  inflationProfiles: InflationProfile[];
  activeInflationProfile: string | undefined;
  onActivateInflation?: (profile: InflationProfile) => void;
  onChanged: () => void;
}) {
  const [section, setSection] = useState<PortfolioSection>("accounts");

  return (
    <>
      <SubTabBar
        ariaLabel="Portfolio section"
        options={SECTIONS}
        value={section}
        onChange={setSection}
        caption={CAPTIONS[section]}
      />
      {section === "accounts" ? (
        <AccountsScreen
          scenarioId={scenarioId}
          accounts={accounts}
          raw={raw}
          onChanged={onChanged}
        />
      ) : (
        <AssetsReturnsScreen
          scenarioId={scenarioId}
          raw={raw}
          returnProfiles={returnProfiles}
          inflationProfiles={inflationProfiles}
          activeInflationProfile={activeInflationProfile}
          onActivateInflation={onActivateInflation}
          onChanged={onChanged}
        />
      )}
    </>
  );
}
