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
