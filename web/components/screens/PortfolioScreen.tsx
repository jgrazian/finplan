"use client";

import { SubTabBar } from "@/components/layout";
import type { SegmentOption } from "@/components/ui";
import type { RawWorkspace } from "@/lib/hooks/useWorkspace";
import { useNav } from "@/lib/nav";
import type { Account, InflationProfile, ReturnProfile } from "@/lib/types";
import { AccountsScreen } from "./AccountsScreen";
import { AssetsScreen } from "./AssetsScreen";

type PortfolioSection = "accounts" | "returns";

// The value stays `returns` though the label no longer says so: it is in every
// URL anyone has bookmarked, and renaming it would break those to no end.
const SECTIONS: ReadonlyArray<SegmentOption<PortfolioSection>> = [
  { value: "accounts", label: "Accounts" },
  { value: "returns", label: "Assets" },
];

const CAPTIONS: Record<PortfolioSection, string | undefined> = {
  accounts: undefined,
  returns:
    "Holdings are the list. A return profile is what a holding points at, so it is a column — and a library of its own further down.",
};

/** Portfolio tab: the scenario's accounts, and the assets they hold. */
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
        <AssetsScreen
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
