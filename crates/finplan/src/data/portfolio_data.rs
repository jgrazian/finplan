use serde::{Deserialize, Serialize};

use crate::data::profiles_data::ReturnProfileTag;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AssetTag(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioData {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub accounts: Vec<AccountData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Property {
    pub value: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_profile: Option<ReturnProfileTag>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Debt {
    pub balance: f64,
    pub interest_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetAccount {
    pub assets: Vec<AssetValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetValue {
    pub asset: AssetTag,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AccountType {
    Brokerage(AssetAccount),
    Traditional401k(AssetAccount),
    Roth401k(AssetAccount),
    TraditionalIRA(AssetAccount),
    RothIRA(AssetAccount),
    Checking(Property),
    Savings(Property),
    HSA(Property),
    Property(Property),
    Collectible(Property),
    Mortgage(Debt),
    LoanDebt(Debt),
    StudentLoanDebt(Debt),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountData {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(flatten)]
    pub account_type: AccountType,
}

/// Categories of account types for grouping in UI and logic
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountCategory {
    Investment,
    Cash,
    Debt,
}

impl AccountCategory {
    pub fn display_name(&self) -> &'static str {
        match self {
            AccountCategory::Investment => "Investment",
            AccountCategory::Cash => "Cash/Property",
            AccountCategory::Debt => "Debt",
        }
    }

    pub fn all() -> &'static [AccountCategory] {
        &[
            AccountCategory::Investment,
            AccountCategory::Cash,
            AccountCategory::Debt,
        ]
    }
}

impl AccountType {
    /// Returns the category this account type belongs to
    pub fn category(&self) -> AccountCategory {
        match self {
            AccountType::Brokerage(_)
            | AccountType::Traditional401k(_)
            | AccountType::Roth401k(_)
            | AccountType::TraditionalIRA(_)
            | AccountType::RothIRA(_) => AccountCategory::Investment,
            AccountType::Checking(_)
            | AccountType::Savings(_)
            | AccountType::HSA(_)
            | AccountType::Property(_)
            | AccountType::Collectible(_) => AccountCategory::Cash,
            AccountType::Mortgage(_)
            | AccountType::LoanDebt(_)
            | AccountType::StudentLoanDebt(_) => AccountCategory::Debt,
        }
    }

    /// Returns the display name for this account type
    pub fn display_name(&self) -> &'static str {
        match self {
            AccountType::Brokerage(_) => "Brokerage",
            AccountType::Traditional401k(_) => "Traditional 401k",
            AccountType::Roth401k(_) => "Roth 401k",
            AccountType::TraditionalIRA(_) => "Traditional IRA",
            AccountType::RothIRA(_) => "Roth IRA",
            AccountType::Checking(_) => "Checking",
            AccountType::Savings(_) => "Savings",
            AccountType::HSA(_) => "HSA",
            AccountType::Property(_) => "Property",
            AccountType::Collectible(_) => "Collectible",
            AccountType::Mortgage(_) => "Mortgage",
            AccountType::LoanDebt(_) => "Loan",
            AccountType::StudentLoanDebt(_) => "Student Loan",
        }
    }

    /// Returns a reference to the inner AssetAccount if this is an investment account
    pub fn as_investment(&self) -> Option<&AssetAccount> {
        match self {
            AccountType::Brokerage(inv)
            | AccountType::Traditional401k(inv)
            | AccountType::Roth401k(inv)
            | AccountType::TraditionalIRA(inv)
            | AccountType::RothIRA(inv) => Some(inv),
            _ => None,
        }
    }

    /// Returns a mutable reference to the inner AssetAccount if this is an investment account
    pub fn as_investment_mut(&mut self) -> Option<&mut AssetAccount> {
        match self {
            AccountType::Brokerage(inv)
            | AccountType::Traditional401k(inv)
            | AccountType::Roth401k(inv)
            | AccountType::TraditionalIRA(inv)
            | AccountType::RothIRA(inv) => Some(inv),
            _ => None,
        }
    }

    /// Returns a reference to the inner Property if this is a cash/property account
    pub fn as_property(&self) -> Option<&Property> {
        match self {
            AccountType::Checking(prop)
            | AccountType::Savings(prop)
            | AccountType::HSA(prop)
            | AccountType::Property(prop)
            | AccountType::Collectible(prop) => Some(prop),
            _ => None,
        }
    }

    /// Returns a mutable reference to the inner Property if this is a cash/property account
    pub fn as_property_mut(&mut self) -> Option<&mut Property> {
        match self {
            AccountType::Checking(prop)
            | AccountType::Savings(prop)
            | AccountType::HSA(prop)
            | AccountType::Property(prop)
            | AccountType::Collectible(prop) => Some(prop),
            _ => None,
        }
    }

    /// Returns a reference to the inner Debt if this is a debt account
    pub fn as_debt(&self) -> Option<&Debt> {
        match self {
            AccountType::Mortgage(debt)
            | AccountType::LoanDebt(debt)
            | AccountType::StudentLoanDebt(debt) => Some(debt),
            _ => None,
        }
    }

    /// Returns a mutable reference to the inner Debt if this is a debt account
    pub fn as_debt_mut(&mut self) -> Option<&mut Debt> {
        match self {
            AccountType::Mortgage(debt)
            | AccountType::LoanDebt(debt)
            | AccountType::StudentLoanDebt(debt) => Some(debt),
            _ => None,
        }
    }

    /// Returns true if this account type can hold assets (holdings)
    pub fn can_hold_assets(&self) -> bool {
        self.as_investment().is_some()
    }
}
