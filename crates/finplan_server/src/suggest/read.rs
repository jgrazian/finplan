//! A target's current body, built from a `ScenarioGraph` in exactly the shape
//! its GET route returns — the shape a `Change` path points into.

use serde_json::Value;

use crate::api::accounts::{Account, ContributionPeriod, FlavorSpec, Position, TaxStatus};
use crate::api::assets::Asset;
use crate::compile::rows::ScenarioGraph;

/// `GET /scenarios/{id}/events/{event}`, or `None` when the graph has no such
/// event (or cannot read it back).
pub(crate) fn event(graph: &ScenarioGraph, id: i64) -> Option<Value> {
    let event = crate::api::events::read_event(graph, id).ok()?;
    serde_json::to_value(event).ok()
}

/// `GET /scenarios/{id}/assets/{asset}`.
pub(crate) fn asset(graph: &ScenarioGraph, id: i64) -> Option<Value> {
    let row = graph.assets.iter().find(|a| a.id == id)?;
    serde_json::to_value(Asset {
        id: row.id,
        name: row.name.clone(),
        description: row.description.clone(),
        initial_price: row.initial_price,
        return_profile_id: row.return_profile_id,
        tracking_error: row.tracking_error,
        sort_order: row.sort_order,
    })
    .ok()
}

/// `GET /scenarios/{id}/accounts/{account}`: the account, its flavor fields
/// flattened in beside it, and its lots.
pub(crate) fn account(graph: &ScenarioGraph, id: i64) -> Option<Value> {
    let row = graph.accounts.iter().find(|a| a.id == id)?;
    let flavor = match row.flavor.as_str() {
        "Bank" => {
            let bank = graph.bank.get(&id)?;
            FlavorSpec::Bank {
                cash_value: bank.cash_value,
                return_profile_id: bank.return_profile_id,
            }
        }
        "Investment" => {
            let inv = graph.investment.get(&id)?;
            FlavorSpec::Investment {
                tax_status: match inv.tax_status.as_str() {
                    "TaxDeferred" => TaxStatus::TaxDeferred,
                    "TaxFree" => TaxStatus::TaxFree,
                    _ => TaxStatus::Taxable,
                },
                cash_value: inv.cash_value,
                cash_return_profile_id: inv.cash_return_profile_id,
                contribution_limit: inv.contribution_limit,
                contribution_period: match inv.contribution_period.as_deref() {
                    Some("Monthly") => Some(ContributionPeriod::Monthly),
                    Some("Yearly") => Some(ContributionPeriod::Yearly),
                    _ => None,
                },
            }
        }
        "Property" => {
            let property = graph.property.get(&id)?;
            FlavorSpec::Property {
                asset_id: property.asset_id,
                value: property.value,
            }
        }
        _ => {
            let loan = graph.liability.get(&id)?;
            FlavorSpec::Liability {
                principal: loan.principal,
                interest_rate: loan.interest_rate,
                repayment: crate::api::accounts::repayment_of(
                    loan.repay_from_account_id,
                    loan.term_months,
                ),
            }
        }
    };
    let positions = graph
        .positions
        .get(&id)
        .into_iter()
        .flatten()
        .map(|p| Position {
            id: p.id,
            asset_id: p.asset_id,
            purchase_date: p.purchase_date.clone(),
            units: p.units,
            cost_basis: p.cost_basis,
        })
        .collect();
    serde_json::to_value(Account {
        id: row.id,
        name: row.name.clone(),
        description: row.description.clone(),
        sort_order: row.sort_order,
        flavor,
        positions,
    })
    .ok()
}
