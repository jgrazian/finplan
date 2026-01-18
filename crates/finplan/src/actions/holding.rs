// Holding actions - CRUD operations for asset holdings in investment accounts

use crate::data::portfolio_data::{AccountType, AssetTag, AssetValue};
use crate::modals::parse_currency;
use crate::state::AppState;

use super::{ActionContext, ActionResult};

/// Handle adding a holding to an investment account
pub fn handle_add_holding(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let idx = match ctx.index() {
        Some(i) => i,
        None => return ActionResult::close(),
    };

    let parts = ctx.value_parts();
    let asset_name = parts.first().unwrap_or(&"").to_string();
    let asset_value = parts
        .get(1)
        .and_then(|s| parse_currency(s).ok())
        .unwrap_or(0.0);

    if asset_name.is_empty() {
        return ActionResult::error("Asset name cannot be empty");
    }

    if let Some(account) = state.data_mut().portfolios.accounts.get_mut(idx) {
        let assets = match &mut account.account_type {
            AccountType::Brokerage(inv)
            | AccountType::Traditional401k(inv)
            | AccountType::Roth401k(inv)
            | AccountType::TraditionalIRA(inv)
            | AccountType::RothIRA(inv) => Some(&mut inv.assets),
            _ => None,
        };

        if let Some(assets) = assets {
            assets.push(AssetValue {
                asset: AssetTag(asset_name),
                value: asset_value,
            });
            return ActionResult::modified();
        }
    }

    ActionResult::close()
}

/// Handle editing a holding
pub fn handle_edit_holding(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    // Context format: "account_idx:holding_idx"
    let indices = ctx.indices();

    if indices.len() != 2 {
        return ActionResult::close();
    }

    let (account_idx, holding_idx) = (indices[0], indices[1]);
    let parts = ctx.value_parts();

    if let Some(account) = state
        .data_mut()
        .portfolios
        .accounts
        .get_mut(account_idx)
    {
        let assets = match &mut account.account_type {
            AccountType::Brokerage(inv)
            | AccountType::Traditional401k(inv)
            | AccountType::Roth401k(inv)
            | AccountType::TraditionalIRA(inv)
            | AccountType::RothIRA(inv) => Some(&mut inv.assets),
            _ => None,
        };

        if let Some(assets) = assets
            && let Some(holding) = assets.get_mut(holding_idx)
        {
            if let Some(name) = parts.first() {
                holding.asset = AssetTag(name.to_string());
            }
            if let Some(val) = parts.get(1).and_then(|s| parse_currency(s).ok()) {
                holding.value = val;
            }
            return ActionResult::modified();
        }
    }

    ActionResult::close()
}

/// Handle deleting a holding
pub fn handle_delete_holding(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    // Context format: "account_idx:holding_idx"
    let indices = ctx.indices();

    if indices.len() != 2 {
        return ActionResult::close();
    }

    let (account_idx, holding_idx) = (indices[0], indices[1]);

    if let Some(account) = state
        .data_mut()
        .portfolios
        .accounts
        .get_mut(account_idx)
    {
        let assets = match &mut account.account_type {
            AccountType::Brokerage(inv)
            | AccountType::Traditional401k(inv)
            | AccountType::Roth401k(inv)
            | AccountType::TraditionalIRA(inv)
            | AccountType::RothIRA(inv) => Some(&mut inv.assets),
            _ => None,
        };

        if let Some(assets) = assets
            && holding_idx < assets.len()
        {
            assets.remove(holding_idx);
            return ActionResult::modified();
        }
    }

    ActionResult::close()
}
