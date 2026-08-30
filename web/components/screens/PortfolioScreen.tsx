"use client";

import { useState } from "react";
import { SubTabBar } from "@/components/layout";
import type { SegmentOption } from "@/components/ui";
import { AccountsScreen } from "./AccountsScreen";
import { ProfilesScreen } from "./ProfilesScreen";

type PortfolioSection = "accounts" | "profiles";

const SECTIONS: ReadonlyArray<SegmentOption<PortfolioSection>> = [
  { value: "accounts", label: "Accounts" },
  { value: "profiles", label: "Profiles" },
];

const CAPTIONS: Record<PortfolioSection, string | undefined> = {
  accounts: undefined,
  profiles: "Return behaviour is data, not code — every asset points at one of these.",
};

/** Portfolio tab: accounts (1d) and return/inflation profiles (3c). */
export function PortfolioScreen() {
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
      {section === "accounts" ? <AccountsScreen /> : <ProfilesScreen />}
    </>
  );
}
