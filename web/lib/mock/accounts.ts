import type { Account } from "@/lib/types";

/**
 * Fixtures lifted from the canvas script's `accounts` array, re-expressed in
 * the engine's own shape: signed balances, structured lots, real tax status.
 */
export const MOCK_ACCOUNTS: Account[] = [
  {
    accountId: "401k",
    name: "401(k)",
    flavor: "Investment",
    taxStatus: "TaxDeferred",
    balance: 563_400,
    returnProfileId: "us-60-40",
    contributionLimit: { amount: 23_500, period: "Yearly" },
    positions: [
      { assetId: "VTI", purchaseDate: "2019-03-04", units: 412.5, costBasis: 88_100 },
      { assetId: "VXUS", purchaseDate: "2020-06-15", units: 901.0, costBasis: 52_400 },
      { assetId: "BND", purchaseDate: "2021-01-11", units: 1_240.0, costBasis: 91_800 },
    ],
    referencedBy: ["salary-contribution", "rmd-age-75", "retirement-sweep"],
  },
  {
    accountId: "brokerage",
    name: "Brokerage",
    flavor: "Investment",
    taxStatus: "Taxable",
    balance: 412_900,
    returnProfileId: "us-equity-hist",
    positions: [
      { assetId: "VTI", purchaseDate: "2018-08-22", units: 780.0, costBasis: 196_400 },
      { assetId: "AVUV", purchaseDate: "2022-02-07", units: 210.0, costBasis: 61_900 },
    ],
    referencedBy: ["retirement-sweep"],
  },
  {
    accountId: "roth-ira",
    name: "Roth IRA",
    flavor: "Investment",
    taxStatus: "TaxFree",
    balance: 179_200,
    returnProfileId: "us-equity-hist",
    contributionLimit: { amount: 7_000, period: "Yearly" },
    positions: [
      { assetId: "VTI", purchaseDate: "2017-05-30", units: 318.0, costBasis: 71_200 },
    ],
    referencedBy: ["retirement-sweep"],
  },
  {
    accountId: "hysa",
    name: "HYSA",
    flavor: "Bank",
    taxStatus: "Taxable",
    balance: 61_000,
    returnProfileId: "cash-4pct",
    positions: [],
    referencedBy: ["salary", "living-expenses", "retirement-sweep", "social-security"],
  },
  {
    accountId: "home",
    name: "Home",
    flavor: "Property",
    balance: 540_000,
    returnProfileId: "housing-hist",
    positions: [],
    referencedBy: ["mortgage-payoff"],
  },
  {
    accountId: "mortgage",
    name: "Mortgage",
    flavor: "Liability",
    balance: -288_500,
    returnProfileId: "fixed-6.1pct",
    positions: [],
    referencedBy: ["mortgage-payment", "mortgage-payoff"],
  },
];

/** Share of gross (positive-value) holdings; liabilities get no share. */
export function accountShares(accounts: Account[]): Map<string, number | null> {
  const gross = accounts
    .filter((a) => a.balance > 0)
    .reduce((sum, a) => sum + a.balance, 0);
  return new Map(
    accounts.map((a) => [a.accountId, a.balance > 0 ? a.balance / gross : null]),
  );
}
