//! What is read out of a document besides its text: the institution, and per
//! account the balances, positions and transactions. Every parser (OFX, CSV,
//! statement text) fills these, and the document tools work over them, so a
//! tool never cares which format a figure came from.
//!
//! Account identifiers are already reduced to their last four digits here.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DocumentData {
    pub institution: Option<String>,
    pub accounts: Vec<AccountData>,
}

impl DocumentData {
    pub fn is_empty(&self) -> bool {
        self.institution.is_none() && self.accounts.iter().all(AccountData::is_empty)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AccountData {
    /// `checking`, `savings`, `credit_card`, `investment`, `retirement`, ...
    /// as the document says; `None` when it does not.
    pub kind: Option<String>,
    pub last4: Option<String>,
    pub currency: Option<String>,
    pub balances: Vec<Balance>,
    pub positions: Vec<Position>,
    pub transactions: Vec<Transaction>,
}

impl AccountData {
    pub fn is_empty(&self) -> bool {
        self.last4.is_none()
            && self.balances.is_empty()
            && self.positions.is_empty()
            && self.transactions.is_empty()
    }

    /// The figure that stands for the account's value: a labelled total,
    /// ending or ledger balance first, else the market value of its positions
    /// plus any cash balance.
    pub fn headline_balance(&self) -> Option<f64> {
        const ORDER: [&str; 6] = ["total", "ending", "closing", "ledger", "current", "balance"];
        for wanted in ORDER {
            if let Some(b) = self.balances.iter().find(|b| b.label == wanted) {
                return Some(b.amount);
            }
        }
        let positions: f64 = self.positions.iter().filter_map(|p| p.market_value).sum();
        let has_positions = self.positions.iter().any(|p| p.market_value.is_some());
        let cash = self.cash_balance();
        match (has_positions, cash) {
            (true, cash) => Some(positions + cash.unwrap_or(0.0)),
            (false, Some(cash)) => Some(cash),
            (false, None) => self.balances.first().map(|b| b.amount),
        }
    }

    pub fn cash_balance(&self) -> Option<f64> {
        self.balances
            .iter()
            .find(|b| b.label == "cash")
            .map(|b| b.amount)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Balance {
    /// `ledger`, `available`, `cash`, `ending`, `total`, ...
    pub label: String,
    pub amount: f64,
    /// ISO date, when the document says when.
    pub as_of: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Position {
    pub symbol: Option<String>,
    pub name: Option<String>,
    pub units: f64,
    pub unit_price: Option<f64>,
    pub market_value: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Transaction {
    /// ISO date.
    pub date: String,
    /// Negative for money out.
    pub amount: f64,
    pub description: String,
}

/// The last four digits of an identifier, when it has that many.
pub fn last_four(id: &str) -> Option<String> {
    let digits: Vec<char> = id.chars().filter(char::is_ascii_digit).collect();
    (digits.len() >= 4).then(|| digits[digits.len() - 4..].iter().collect())
}
