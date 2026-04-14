// Ticker-to-profile suggestions mirroring crates/finplan/src/data/ticker_profiles.rs.
// When a user types a known ticker like "VTI", suggest a Normal profile with the
// matching historical mean / std-dev so they don't have to enter it by hand.

import type { ReturnProfileType } from "./types";

export interface ProfileSuggestion {
  profile_name: string;
  profile_type: ReturnProfileType;
  mean: number;
  std_dev: number;
}

interface ProfileCategory extends ProfileSuggestion {
  tickers: readonly string[];
}

export const PROFILE_CATEGORIES: readonly ProfileCategory[] = [
  {
    profile_name: "US Total Market",
    profile_type: "Normal",
    mean: 0.11471,
    std_dev: 0.18146,
    tickers: ["VTI", "VTSAX", "ITOT", "SPTM", "SCHB", "FSKAX", "FZROX"],
  },
  {
    profile_name: "S&P 500",
    profile_type: "Normal",
    mean: 0.11471,
    std_dev: 0.18146,
    tickers: ["VOO", "SPY", "IVV", "VFIAX", "FXAIX", "SWPPX"],
  },
  {
    profile_name: "US Small Cap",
    profile_type: "Normal",
    mean: 0.147749,
    std_dev: 0.278003,
    tickers: ["VB", "IJR", "SCHA", "VBR", "IWM", "VIOO", "VSMAX"],
  },
  {
    profile_name: "US Aggregate Bond",
    profile_type: "Normal",
    mean: 0.0311818,
    std_dev: 0.0468972,
    tickers: ["BND", "AGG", "VBTLX", "SCHZ", "FBND", "FXNAX"],
  },
  {
    profile_name: "International Developed",
    profile_type: "Normal",
    mean: 0.0778324,
    std_dev: 0.188273,
    tickers: ["VXUS", "VEA", "EFA", "IXUS", "IEFA", "SWISX", "FSPSX"],
  },
  {
    profile_name: "Emerging Markets",
    profile_type: "Normal",
    mean: 0.107264,
    std_dev: 0.347473,
    tickers: ["VWO", "IEMG", "EEM", "SCHE", "VEMAX"],
  },
  {
    profile_name: "REITs",
    profile_type: "Normal",
    mean: 0.082752,
    std_dev: 0.195905,
    tickers: ["VNQ", "IYR", "SCHH", "FREL", "VGSLX", "RWR"],
  },
  {
    profile_name: "Money Market",
    profile_type: "Normal",
    mean: 0.0341782,
    std_dev: 0.0305423,
    tickers: ["VGSH", "SHV", "BIL", "VMFXX", "SPAXX", "FDRXX", "SGOV"],
  },
  {
    profile_name: "Long-Term Treasury",
    profile_type: "Normal",
    mean: 0.047717,
    std_dev: 0.0700793,
    tickers: ["TLT", "VGLT", "EDV", "SPTL", "ZROZ"],
  },
  {
    profile_name: "TIPS",
    profile_type: "Normal",
    mean: 0.0358924,
    std_dev: 0.0606518,
    tickers: ["TIP", "VTIP", "SCHP", "STIP", "FIPDX"],
  },
  {
    profile_name: "US Corporate Bond",
    profile_type: "Normal",
    mean: 0.0441447,
    std_dev: 0.0697513,
    tickers: ["LQD", "VCIT", "IGIB", "SPIB", "VCSH"],
  },
  {
    profile_name: "Gold",
    profile_type: "Normal",
    mean: 0.131744,
    std_dev: 0.173436,
    tickers: ["GLD", "IAU", "SGOL", "GLDM"],
  },
];

export function getSuggestion(ticker: string): ProfileSuggestion | null {
  const upper = ticker.trim().toUpperCase();
  if (!upper) return null;
  const hit = PROFILE_CATEGORIES.find((c) =>
    c.tickers.some((t) => t === upper)
  );
  if (!hit) return null;
  return {
    profile_name: hit.profile_name,
    profile_type: hit.profile_type,
    mean: hit.mean,
    std_dev: hit.std_dev,
  };
}
