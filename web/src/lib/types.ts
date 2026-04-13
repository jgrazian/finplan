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
