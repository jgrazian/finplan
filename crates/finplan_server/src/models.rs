use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// All valid account types mapped to their category.
pub fn category_for_type(account_type: &str) -> Option<&'static str> {
    match account_type {
        "Brokerage" | "Traditional401k" | "Roth401k" | "TraditionalIRA" | "RothIRA" => {
            Some("Investment")
        }
        "Checking" | "Savings" | "HSA" | "Property" | "Collectible" => Some("Cash"),
        "Mortgage" | "LoanDebt" | "StudentLoanDebt" => Some("Debt"),
        _ => None,
    }
}

// -- Database row types --

#[derive(Debug, FromRow)]
#[allow(dead_code)]
pub struct AccountRow {
    pub id: i64,
    pub user_id: String,
    pub name: String,
    pub description: Option<String>,
    pub account_type: String,
    pub category: String,
    pub value: Option<f64>,
    pub return_profile: Option<String>,
    pub balance: Option<f64>,
    pub interest_rate: Option<f64>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct HoldingRow {
    pub id: i64,
    pub account_id: i64,
    pub asset_name: String,
    pub value: f64,
}

// -- API response/request types --

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountResponse {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub account_type: String,
    pub category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interest_rate: Option<f64>,
    pub total_value: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holdings: Option<Vec<HoldingRow>>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAccountRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub account_type: String,
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub return_profile: Option<String>,
    #[serde(default)]
    pub balance: Option<f64>,
    #[serde(default)]
    pub interest_rate: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAccountRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub return_profile: Option<String>,
    #[serde(default)]
    pub balance: Option<f64>,
    #[serde(default)]
    pub interest_rate: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateHoldingRequest {
    pub asset_name: String,
    pub value: f64,
}

#[derive(Debug, Deserialize)]
pub struct UpdateHoldingRequest {
    #[serde(default)]
    pub asset_name: Option<String>,
    #[serde(default)]
    pub value: Option<f64>,
}

// -- Allocation types --

#[derive(Debug, Serialize)]
pub struct AllocationSlice {
    pub name: String,
    pub value: f64,
    pub percentage: f64,
}

#[derive(Debug, Serialize)]
pub struct AllocationResponse {
    pub total_value: f64,
    pub by_account: Vec<AllocationSlice>,
    pub by_asset: Vec<AllocationSlice>,
    pub by_category: Vec<AllocationSlice>,
}

impl AccountRow {
    pub fn into_response(self, holdings: Option<Vec<HoldingRow>>) -> AccountResponse {
        let total = compute_total(
            &self.category,
            self.value,
            self.balance,
            holdings.as_deref().unwrap_or(&[]),
        );
        AccountResponse {
            id: self.id,
            name: self.name,
            description: self.description,
            account_type: self.account_type,
            category: self.category,
            value: self.value,
            return_profile: self.return_profile,
            balance: self.balance,
            interest_rate: self.interest_rate,
            total_value: total,
            holdings,
        }
    }
}

fn compute_total(
    category: &str,
    value: Option<f64>,
    balance: Option<f64>,
    holdings: &[HoldingRow],
) -> f64 {
    match category {
        "Investment" => holdings.iter().map(|h| h.value).sum(),
        "Cash" => value.unwrap_or(0.0),
        "Debt" => -(balance.unwrap_or(0.0).abs()),
        _ => 0.0,
    }
}

// -- Return profile types --

#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
pub struct ReturnProfileRow {
    pub id: i64,
    pub user_id: String,
    pub name: String,
    pub description: Option<String>,
    pub profile_type: String,
    pub rate: Option<f64>,
    pub mean: Option<f64>,
    pub std_dev: Option<f64>,
    pub scale: Option<f64>,
    pub df: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReturnProfileResponse {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub profile_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub std_dev: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub df: Option<f64>,
}

impl From<ReturnProfileRow> for ReturnProfileResponse {
    fn from(row: ReturnProfileRow) -> Self {
        ReturnProfileResponse {
            id: row.id,
            name: row.name,
            description: row.description,
            profile_type: row.profile_type,
            rate: row.rate,
            mean: row.mean,
            std_dev: row.std_dev,
            scale: row.scale,
            df: row.df,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ProfileRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub profile_type: String,
    #[serde(default)]
    pub rate: Option<f64>,
    #[serde(default)]
    pub mean: Option<f64>,
    #[serde(default)]
    pub std_dev: Option<f64>,
    #[serde(default)]
    pub scale: Option<f64>,
    #[serde(default)]
    pub df: Option<f64>,
}

/// Valid profile types accepted by the API.
pub const VALID_PROFILE_TYPES: &[&str] = &["None", "Fixed", "Normal", "LogNormal", "StudentT"];

pub fn is_valid_profile_type(t: &str) -> bool {
    VALID_PROFILE_TYPES.contains(&t)
}

// -- Asset mapping types --

#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
pub struct AssetMappingRow {
    pub id: i64,
    pub user_id: String,
    pub asset_name: String,
    pub profile_id: i64,
}

#[derive(Debug, Serialize)]
pub struct AssetMappingResponse {
    pub asset_name: String,
    pub profile_id: i64,
    pub profile_name: String,
}

#[derive(Debug, Deserialize)]
pub struct UpsertMappingRequest {
    pub profile_id: i64,
}

// -- Auth types --

#[derive(Debug, Deserialize)]
pub struct AuthRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub id: String,
    pub email: String,
}
