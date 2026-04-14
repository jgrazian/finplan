export interface User {
  id: string;
  email: string;
}

export type AccountCategory = "Investment" | "Cash" | "Debt";

export type AccountType =
  | "Brokerage"
  | "Traditional401k"
  | "Roth401k"
  | "TraditionalIRA"
  | "RothIRA"
  | "Checking"
  | "Savings"
  | "HSA"
  | "Property"
  | "Collectible"
  | "Mortgage"
  | "LoanDebt"
  | "StudentLoanDebt";

export const ACCOUNT_TYPE_DISPLAY: Record<AccountType, string> = {
  Brokerage: "Brokerage",
  Traditional401k: "Traditional 401k",
  Roth401k: "Roth 401k",
  TraditionalIRA: "Traditional IRA",
  RothIRA: "Roth IRA",
  Checking: "Checking",
  Savings: "Savings",
  HSA: "HSA",
  Property: "Property",
  Collectible: "Collectible",
  Mortgage: "Mortgage",
  LoanDebt: "Loan",
  StudentLoanDebt: "Student Loan",
};

export const CATEGORY_TYPES: Record<AccountCategory, AccountType[]> = {
  Investment: [
    "Brokerage",
    "Traditional401k",
    "Roth401k",
    "TraditionalIRA",
    "RothIRA",
  ],
  Cash: ["Checking", "Savings", "HSA", "Property", "Collectible"],
  Debt: ["Mortgage", "LoanDebt", "StudentLoanDebt"],
};

export interface Holding {
  id: number;
  account_id: number;
  asset_name: string;
  value: number;
}

export interface Account {
  id: number;
  name: string;
  description?: string;
  account_type: AccountType;
  category: AccountCategory;
  value?: number;
  return_profile?: string;
  balance?: number;
  interest_rate?: number;
  total_value: number;
  holdings?: Holding[];
}

export type ReturnProfileType =
  | "None"
  | "Fixed"
  | "Normal"
  | "LogNormal"
  | "StudentT"
  | "Bootstrap";

export const RETURN_PROFILE_TYPE_LABELS: Record<ReturnProfileType, string> = {
  None: "None (0%)",
  Fixed: "Fixed Rate",
  Normal: "Normal Distribution",
  LogNormal: "Log-Normal Distribution",
  StudentT: "Student's t (Fat Tails)",
  Bootstrap: "Historical (Bootstrap)",
};

export interface ReturnProfile {
  id: number;
  name: string;
  description?: string;
  profile_type: ReturnProfileType;
  rate?: number;
  mean?: number;
  std_dev?: number;
  scale?: number;
  df?: number;
  preset?: string;
  block_size?: number;
}

/// Historical preset metadata - mirrors HISTORICAL_PRESETS in ticker_profiles.rs.
export interface HistoricalPreset {
  key: string;
  name: string;
  description: string;
}

export const HISTORICAL_PRESETS: readonly HistoricalPreset[] = [
  { key: "sp500", name: "S&P 500", description: "US Large Cap (1927-2023, 97yr)" },
  { key: "us_small_cap", name: "US Small Cap", description: "Small Cap Stocks (1927-2024, 98yr)" },
  { key: "us_tbills", name: "US T-Bills", description: "3-Month Treasury (1934-2025, 92yr)" },
  { key: "us_long_bonds", name: "US Long Bonds", description: "Long-Term Gov Bonds (1927-2023, 97yr)" },
  { key: "intl_developed", name: "Intl Developed", description: "Developed ex-US (1991-2024, 34yr)" },
  { key: "emerging_markets", name: "Emerging Markets", description: "EM Stocks (1991-2024, 33yr)" },
  { key: "reits", name: "REITs", description: "Real Estate Trusts (2005-2026, 22yr)" },
  { key: "gold", name: "Gold", description: "Gold (2001-2026, 26yr)" },
  { key: "us_agg_bonds", name: "US Agg Bonds", description: "Aggregate Bonds (2004-2026, 23yr)" },
  { key: "us_corporate_bonds", name: "US Corp Bonds", description: "Corporate Bonds (2003-2026, 24yr)" },
  { key: "tips", name: "TIPS", description: "Inflation-Protected (2004-2026, 23yr)" },
];

export function presetLabel(key: string): string {
  return HISTORICAL_PRESETS.find((p) => p.key === key)?.name ?? key;
}

export interface AssetMapping {
  asset_name: string;
  profile_id: number;
  profile_name: string;
}

export interface AllocationSlice {
  name: string;
  value: number;
  percentage: number;
}

export interface AllocationData {
  total_value: number;
  by_account: AllocationSlice[];
  by_asset: AllocationSlice[];
  by_category: AllocationSlice[];
}
