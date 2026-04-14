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
  | "StudentT";

export const RETURN_PROFILE_TYPE_LABELS: Record<ReturnProfileType, string> = {
  None: "None (0%)",
  Fixed: "Fixed Rate",
  Normal: "Normal Distribution",
  LogNormal: "Log-Normal Distribution",
  StudentT: "Student's t (Fat Tails)",
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
