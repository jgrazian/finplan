"use client";

import { SubTabBar } from "@/components/layout";
import type { SegmentOption } from "@/components/ui";
import type { RawWorkspace } from "@/lib/hooks/useWorkspace";
import { useNav } from "@/lib/nav";
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
  offline,
}: {
  scenarioId: number;
  accounts: Account[];
  raw: RawWorkspace;
  returnProfiles: ReturnProfile[];
  inflationProfiles: InflationProfile[];
  activeInflationProfile: string | undefined;
  onActivateInflation?: (profile: InflationProfile) => void;
  onChanged: () => void;
  /** Writes are being refused: add and delete cannot be offered. */
  offline?: boolean;
}) {
  // The sub-tab is in the query, so a refresh comes back to the same half of
  // the tab. Anything else in `sec` reads as the default rather than as an
  // error: a hand-edited URL should land somewhere, not nowhere.
  const nav = useNav();
  const section: PortfolioSection = nav.section === "returns" ? "returns" : "accounts";

  return (
    <>
      <SubTabBar
        ariaLabel="Portfolio section"
        options={SECTIONS}
        value={section}
        onChange={nav.setSection}
        caption={CAPTIONS[section]}
      />
      {section === "accounts" ? (
        <AccountsScreen
          scenarioId={scenarioId}
          accounts={accounts}
          raw={raw}
          onChanged={onChanged}
          offline={offline}
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
          offline={offline}
        />
      )}
    </>
  );
}
