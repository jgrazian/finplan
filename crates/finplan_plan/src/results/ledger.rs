//! Flatten the engine's `LedgerEntry` stream into rows the API can serve.
//!
//! The engine's ledger is an enum of everything that can happen to simulation
//! state. What the Results screen needs from it is much narrower: a date, a
//! short kind label, a line of prose naming the accounts and events involved,
//! and the figures — kept as numbers rather than baked into the prose, so the
//! client can restate them in real dollars.
//!
//! Time advances and year rollovers are dropped. They carry no figure, there
//! is one of each per step, and they would bury everything else.

use finplan_core::model::{AccountId, CashFlowKind, EventId, LedgerEntry, StateEvent};

use crate::compile::CompiledScenario;

/// The four buckets the ledger filter offers.
pub const CASH: &str = "cash";
pub const ASSET: &str = "asset";
pub const TAX: &str = "tax";
pub const EVENT: &str = "event";

/// One ledger entry, flattened for storage.
pub struct LedgerRow {
    pub category: &'static str,
    pub kind: String,
    pub detail: String,
    /// Signed against the plan: money in is positive, money out negative.
    pub amount: Option<f64>,
    /// The gross a tax was charged on, the gain inside a sale, the amount an
    /// RMD required — whatever second figure the entry carries.
    pub basis: Option<f64>,
    pub basis_label: Option<&'static str>,
    pub account_id: Option<i64>,
    pub event_id: Option<i64>,
}

/// Names for the ids an entry references, resolved through the compile's map.
pub struct Names<'a> {
    compiled: &'a CompiledScenario,
}

impl<'a> Names<'a> {
    pub fn new(compiled: &'a CompiledScenario) -> Self {
        Self { compiled }
    }

    fn account_db_id(&self, id: AccountId) -> Option<i64> {
        self.compiled.id_map.account_db_id(id)
    }

    fn event_db_id(&self, id: EventId) -> Option<i64> {
        self.compiled.id_map.event_db_id(id)
    }

    /// An account created mid-simulation has no row to name it, and a name is
    /// better than a dense index the user has never seen.
    fn account(&self, id: AccountId) -> &str {
        self.account_db_id(id)
            .and_then(|db| self.compiled.account_names.get(&db))
            .map_or("an untracked account", String::as_str)
    }

    fn event(&self, id: EventId) -> &str {
        self.event_db_id(id)
            .and_then(|db| self.compiled.event_names.get(&db))
            .map_or("a deleted event", String::as_str)
    }

    /// Rewrite the engine's debug ids in a warning message as names. The
    /// engine only knows dense indices, so its errors read `account
    /// AccountId(3) not found`; users should see the account they named.
    pub fn readable(&self, message: &str) -> String {
        let text = replace_ids(message, "AssetCoord { account_id: AccountId(", |n, rest| {
            // `AssetCoord { account_id: AccountId(3), asset_id: AssetId(1) }`
            let end = rest.find('}').map_or(0, |i| i + 1);
            (
                format!("a holding in “{}”", self.account(AccountId(n))),
                end,
            )
        });
        let text = replace_ids(&text, "AccountId(", |n, _| {
            (format!("“{}”", self.account(AccountId(n))), 0)
        });
        let text = replace_ids(&text, "EventId(", |n, _| {
            (format!("“{}”", self.event(EventId(n))), 0)
        });
        let text = replace_ids(&text, "AssetId(", |_, _| ("an asset".into(), 0));
        let text = replace_ids(&text, "ReturnProfileId(", |_, _| {
            ("a return profile".into(), 0)
        });
        replace_ids(&text, "ParameterId(", |_, _| ("a parameter".into(), 0))
    }

    pub fn account_row(&self, id: AccountId) -> Option<i64> {
        self.account_db_id(id)
    }
}

/// Replace each `prefix<digits>)` with `render(n, rest)`, where `rest` is the
/// text after the `)` and the returned count is how much of it to consume too.
fn replace_ids(text: &str, prefix: &str, render: impl Fn(u16, &str) -> (String, usize)) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(prefix) {
        let after = &rest[start + prefix.len()..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        let id = after[..digits].parse::<u16>().ok();
        match (id, after[digits..].strip_prefix(')')) {
            (Some(n), Some(tail)) => {
                let (name, consumed) = render(n, tail);
                out.push_str(&rest[..start]);
                out.push_str(&name);
                rest = &tail[consumed..];
            }
            _ => {
                out.push_str(&rest[..start + prefix.len()]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The label a cash credit reads under. `CashFlowKind` distinguishes money
/// arriving from outside the plan from money moved inside it, which is the
/// difference between income and a withdrawal.
fn credit_label(kind: CashFlowKind) -> &'static str {
    match kind {
        CashFlowKind::Income => "Income",
        CashFlowKind::LiquidationProceeds => "Withdrawal",
        CashFlowKind::Appreciation => "Interest",
        CashFlowKind::RmdWithdrawal => "RMD",
        CashFlowKind::Transfer => "Transfer in",
        _ => "Credit",
    }
}

fn debit_label(kind: CashFlowKind) -> &'static str {
    match kind {
        CashFlowKind::Expense => "Expense",
        CashFlowKind::Tax => "Tax",
        CashFlowKind::DebtPayment => "Loan payment",
        CashFlowKind::Contribution => "Contribution",
        CashFlowKind::InvestmentPurchase => "Purchase",
        CashFlowKind::Transfer => "Transfer out",
        _ => "Debit",
    }
}

fn row(
    category: &'static str,
    kind: &str,
    detail: String,
    amount: Option<f64>,
    account_id: Option<i64>,
) -> LedgerRow {
    LedgerRow {
        category,
        kind: kind.to_string(),
        detail,
        amount,
        basis: None,
        basis_label: None,
        account_id,
        event_id: None,
    }
}

fn with_basis(mut row: LedgerRow, basis: f64, label: &'static str) -> LedgerRow {
    row.basis = Some(basis);
    row.basis_label = Some(label);
    row
}

/// Flatten one entry, or `None` for the kinds the ledger does not show.
pub fn flatten(entry: &LedgerEntry, names: &Names<'_>) -> Option<LedgerRow> {
    let mut out = match &entry.event {
        // Bookkeeping, not history: one per step, and no figure to read.
        StateEvent::TimeAdvance { .. } | StateEvent::YearRollover { .. } => return None,

        StateEvent::CashCredit { to, amount, kind } => row(
            CASH,
            credit_label(*kind),
            format!("into {}", names.account(*to)),
            Some(*amount),
            names.account_db_id(*to),
        ),

        StateEvent::CashDebit { from, amount, kind } => row(
            CASH,
            debit_label(*kind),
            format!("from {}", names.account(*from)),
            Some(-*amount),
            names.account_db_id(*from),
        ),

        StateEvent::CashAppreciation {
            account_id,
            previous_value,
            new_value,
            return_rate,
            ..
        } => row(
            CASH,
            "Interest",
            format!(
                "{} at {:.2}%",
                names.account(*account_id),
                return_rate * 100.0
            ),
            Some(new_value - previous_value),
            names.account_db_id(*account_id),
        ),

        // Repricing, not trading: the holdings are the same units, worth more
        // or less than they were.
        StateEvent::AssetAppreciation {
            account_id,
            previous_value,
            new_value,
            ..
        } => with_basis(
            row(
                ASSET,
                "Growth",
                format!("holdings in {}", names.account(*account_id)),
                Some(new_value - previous_value),
                names.account_db_id(*account_id),
            ),
            *new_value,
            "worth",
        ),

        StateEvent::LiabilityInterestAccrual {
            account_id,
            previous_principal,
            new_principal,
            interest_rate,
            ..
        } => row(
            CASH,
            "Accrual",
            format!(
                "{} at {:.2}%",
                names.account(*account_id),
                interest_rate * 100.0
            ),
            // Interest on a debt grows what is owed, so it is money out.
            Some(-(new_principal - previous_principal)),
            names.account_db_id(*account_id),
        ),

        StateEvent::AssetPurchase {
            account_id,
            units,
            cost_basis,
            ..
        } => row(
            ASSET,
            "Buy",
            format!("{units:.2} units into {}", names.account(*account_id)),
            Some(-*cost_basis),
            names.account_db_id(*account_id),
        ),

        StateEvent::AssetSale {
            account_id,
            units,
            proceeds,
            short_term_gain,
            long_term_gain,
            ..
        } => with_basis(
            row(
                ASSET,
                "Sell",
                format!("{units:.2} units from {}", names.account(*account_id)),
                Some(*proceeds),
                names.account_db_id(*account_id),
            ),
            short_term_gain + long_term_gain,
            "gain",
        ),

        // Gross, like a sale's proceeds: any tax withheld follows as its own
        // entry, and the net credit after that.
        StateEvent::CashWithdrawal { account_id, amount } => row(
            CASH,
            "Cash withdrawal",
            format!("uninvested cash from {}", names.account(*account_id)),
            Some(*amount),
            names.account_db_id(*account_id),
        ),

        // In kind: no money moves, so the figure is the lot's value with no
        // sign, and nothing reads it as a sale.
        StateEvent::AssetLotMoved {
            from,
            to,
            units,
            value,
            ..
        } => with_basis(
            row(
                ASSET,
                "Move",
                format!(
                    "{units:.2} units from {} to {}",
                    names.account(*from),
                    names.account(*to)
                ),
                None,
                names.account_db_id(*from),
            ),
            *value,
            "worth",
        ),

        // The gross converted, all of it ordinary income; the moves, the
        // tax and any penalty follow as their own entries.
        StateEvent::RothConversion {
            from,
            to,
            amount,
            tax,
            withheld,
        } => with_basis(
            row(
                ASSET,
                "Roth conversion",
                if *withheld > 0.0 {
                    format!(
                        "{} to {}, {:.0} withheld for tax",
                        names.account(*from),
                        names.account(*to),
                        withheld
                    )
                } else {
                    format!("{} to {}", names.account(*from), names.account(*to))
                },
                Some(*amount),
                names.account_db_id(*from),
            ),
            *tax,
            "tax",
        ),

        StateEvent::IncomeTax {
            gross_amount,
            federal_tax,
            state_tax,
            ..
        } => with_basis(
            row(
                TAX,
                "Income tax",
                "federal and state on ordinary income".to_string(),
                Some(-(federal_tax + state_tax)),
                None,
            ),
            *gross_amount,
            "on",
        ),

        StateEvent::ShortTermCapitalGainsTax {
            gross_gain,
            federal_tax,
            state_tax,
        } => with_basis(
            row(
                TAX,
                "Short-term gains tax",
                "on gains held under a year".to_string(),
                Some(-(federal_tax + state_tax)),
                None,
            ),
            *gross_gain,
            "on",
        ),

        StateEvent::LongTermCapitalGainsTax {
            gross_gain,
            federal_tax,
            state_tax,
        } => with_basis(
            row(
                TAX,
                "Long-term gains tax",
                "on gains held over a year".to_string(),
                Some(-(federal_tax + state_tax)),
                None,
            ),
            *gross_gain,
            "on",
        ),

        StateEvent::EarlyWithdrawalPenalty {
            gross_amount,
            penalty_amount,
            penalty_rate,
        } => with_basis(
            row(
                TAX,
                "Penalty",
                format!("early withdrawal, {:.0}%", penalty_rate * 100.0),
                Some(-*penalty_amount),
                None,
            ),
            *gross_amount,
            "on",
        ),

        StateEvent::RmdWithdrawal {
            account_id,
            age,
            required_amount,
            actual_amount,
            ..
        } => with_basis(
            row(
                CASH,
                "RMD",
                format!("{} at age {age}", names.account(*account_id)),
                Some(*actual_amount),
                names.account_db_id(*account_id),
            ),
            *required_amount,
            "required",
        ),

        StateEvent::BalanceAdjusted {
            account,
            delta,
            new_balance,
            ..
        } => row(
            CASH,
            "Adjustment",
            format!("{} now holds {new_balance:.0}", names.account(*account)),
            Some(*delta),
            names.account_db_id(*account),
        ),

        StateEvent::MarketShock { drop, assets } => row(
            ASSET,
            "Market shock",
            format!(
                "prices down {:.0}% on {} market asset(s)",
                drop * 100.0,
                assets.len()
            ),
            None,
            None,
        ),

        StateEvent::CreateAccount(account) => row(
            EVENT,
            "Account opened",
            names.account(account.account_id).to_string(),
            None,
            names.account_db_id(account.account_id),
        ),

        StateEvent::DeleteAccount(id) => row(
            EVENT,
            "Account closed",
            names.account(*id).to_string(),
            None,
            names.account_db_id(*id),
        ),

        StateEvent::EventTriggered { event_id } => row(
            EVENT,
            "Triggered",
            names.event(*event_id).to_string(),
            None,
            None,
        ),
        StateEvent::EventPaused { event_id } => row(
            EVENT,
            "Paused",
            names.event(*event_id).to_string(),
            None,
            None,
        ),
        StateEvent::EventResumed { event_id } => row(
            EVENT,
            "Resumed",
            names.event(*event_id).to_string(),
            None,
            None,
        ),
        StateEvent::EventTerminated { event_id } => row(
            EVENT,
            "Terminated",
            names.event(*event_id).to_string(),
            None,
            None,
        ),
    };

    // The event a state change came from, whether the change names it or the
    // entry carries it as its source.
    out.event_id = entry
        .event
        .event_id()
        .or(entry.source_event)
        .and_then(|id| names.event_db_id(id));

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::replace_ids;

    #[test]
    fn debug_ids_in_messages_are_replaced_and_malformed_ones_kept() {
        let name = |n: u16, _: &str| (format!("#{n}"), 0);
        assert_eq!(
            replace_ids("account AccountId(3) not found", "AccountId(", name),
            "account #3 not found"
        );
        assert_eq!(
            replace_ids("AccountId(1) and AccountId(22)", "AccountId(", name),
            "#1 and #22"
        );
        assert_eq!(
            replace_ids("AccountId(x) AccountId(", "AccountId(", name),
            "AccountId(x) AccountId("
        );
        let coord = replace_ids(
            "asset AssetCoord { account_id: AccountId(4), asset_id: AssetId(1) } not found",
            "AssetCoord { account_id: AccountId(",
            |n, rest| (format!("a holding in #{n}"), rest.find('}').unwrap() + 1),
        );
        assert_eq!(coord, "asset a holding in #4 not found");
    }
}
