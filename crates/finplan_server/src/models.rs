use finplan::config::SimulationConfig;
use finplan::model::{AccountId, AssetId, TaxStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Portfolio Types
// ============================================================================

// Shim types that extend core models with optional name field for frontend display
// These preserve the name through JSON serialization while the core models don't need it

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioAsset {
    pub asset_id: AssetId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_class: Option<String>,
    pub initial_value: f64,
    pub return_profile_index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioAccount {
    pub account_id: AccountId,
    pub tax_status: TaxStatus,
    pub assets: Vec<PortfolioAsset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SavedPortfolio {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub accounts: Vec<PortfolioAccount>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PortfolioListItem {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub total_value: f64,
    pub account_count: usize,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreatePortfolioRequest {
    pub name: String,
    pub description: Option<String>,
    pub accounts: Vec<PortfolioAccount>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePortfolioRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub accounts: Option<Vec<PortfolioAccount>>,
}

#[derive(Debug, Serialize)]
pub struct PortfolioNetworth {
    pub total_value: f64,
    pub by_account_type: HashMap<String, f64>,
    pub by_asset_class: HashMap<String, f64>,
}

// ============================================================================
// Simulation Types
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct SavedSimulation {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub parameters: SimulationConfig,
    pub portfolio_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SimulationListItem {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub portfolio_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSimulationRequest {
    pub name: String,
    pub description: Option<String>,
    pub parameters: SimulationConfig,
    pub portfolio_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSimulationRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub parameters: Option<SimulationConfig>,
    pub portfolio_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RunSimulationRequest {
    #[serde(default = "default_iterations")]
    pub iterations: usize,
}

fn default_iterations() -> usize {
    100
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SimulationRunRecord {
    pub id: String,
    pub simulation_id: String,
    pub iterations: i32,
    pub ran_at: String,
}
