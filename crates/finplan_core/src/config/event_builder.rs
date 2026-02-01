//! Event Builder DSL
//!
//! Provides a fluent API for creating events (income, expenses, transfers, etc.)
//!
//! # Examples
//!
//! ```ignore
//! use finplan::config::EventBuilder;
//!
//! // Monthly salary deposited to checking
//! let salary = EventBuilder::income("Salary")
//!     .to_account("Checking")
//!     .amount(8_000.0)
//!     .monthly()
//!     .until_age(65);
//!
//! // Monthly rent expense
//! let rent = EventBuilder::expense("Rent")
//!     .from_account("Checking")
//!     .amount(2_000.0)
//!     .monthly();
//!
//! // Retirement withdrawal from multiple sources
//! let retirement_income = EventBuilder::withdrawal("Retirement Income")
//!     .to_account("Checking")
//!     .amount(5_000.0)
//!     .net()
//!     .from_accounts_in_order(["Brokerage", "Traditional 401k", "Roth IRA"])
//!     .monthly()
//!     .starting_at_age(65);
//! ```

use crate::model::{
    AccountId, AmountMode, AssetCoord, EventEffect, IncomeType, LotMethod, RepeatInterval,
    TransferAmount, WithdrawalOrder,
};
use jiff::civil::Date;

/// Builder for creating events with a fluent API
#[derive(Debug, Clone)]
pub struct EventBuilder {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) event_type: EventType,
    pub(crate) trigger: TriggerSpec,
    pub(crate) once: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum EventType {
    Income(IncomeSpec),
    Expense(ExpenseSpec),
    AssetPurchase(AssetPurchaseSpec),
    AssetSale(AssetSaleSpec),
    Custom(Vec<EventEffect>),
}

#[derive(Debug, Clone)]
pub(crate) struct IncomeSpec {
    pub to_account: AccountRef,
    pub amount: AmountSpec,
    pub amount_mode: AmountMode,
    pub income_type: IncomeType,
}

#[derive(Debug, Clone)]
pub(crate) struct ExpenseSpec {
    pub from_account: AccountRef,
    pub amount: AmountSpec,
}

#[derive(Debug, Clone)]
pub(crate) struct AssetPurchaseSpec {
    pub from_account: AccountRef,
    pub to_asset: AssetRef,
    pub amount: AmountSpec,
}

#[derive(Debug, Clone)]
pub(crate) struct AssetSaleSpec {
    pub to_account: AccountRef,
    pub amount: AmountSpec,
    pub sources: WithdrawalSourceSpec,
    pub amount_mode: AmountMode,
    pub lot_method: LotMethod,
}

/// Reference to an account - can be by ID or by name (resolved later)
#[derive(Debug, Clone)]
pub enum AccountRef {
    Id(AccountId),
    Name(String),
}

/// Reference to an asset - can be by ID or by name (resolved later)
#[derive(Debug, Clone)]
pub enum AssetRef {
    Coord(AssetCoord),
    Named { account: String, asset: String },
}

/// Specification for transfer amount
#[derive(Debug, Clone)]
pub enum AmountSpec {
    Fixed(f64),
    SourceBalance,
    TransferAmount(TransferAmount),
}

/// Specification for withdrawal sources
#[derive(Debug, Clone)]
pub enum WithdrawalSourceSpec {
    SingleAsset(AssetRef),
    Strategy {
        order: WithdrawalOrder,
        exclude: Vec<AccountRef>,
    },
    AccountOrder(Vec<AccountRef>),
}

/// Trigger specification - resolved to EventTrigger when building
#[derive(Debug, Clone, Default)]
pub enum TriggerSpec {
    #[default]
    Immediate,
    Date(Date),
    Age {
        years: u8,
        months: Option<u8>,
    },
    Repeating {
        interval: RepeatInterval,
        start: Option<Box<TriggerSpec>>,
        end: Option<Box<TriggerSpec>>,
        max_occurrences: Option<u32>,
    },
}

impl EventBuilder {
    // =========================================================================
    // Event Type Constructors
    // =========================================================================

    /// Create an income event (salary, dividends, social security, etc.)
    pub fn income(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            event_type: EventType::Income(IncomeSpec {
                to_account: AccountRef::Name("default".into()),
                amount: AmountSpec::Fixed(0.0),
                amount_mode: AmountMode::Gross,
                income_type: IncomeType::Taxable,
            }),
            trigger: TriggerSpec::Immediate,
            once: false,
        }
    }

    /// Create an expense event (rent, utilities, food, etc.)
    pub fn expense(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            event_type: EventType::Expense(ExpenseSpec {
                from_account: AccountRef::Name("default".into()),
                amount: AmountSpec::Fixed(0.0),
            }),
            trigger: TriggerSpec::Immediate,
            once: false,
        }
    }

    /// Create an asset purchase event (buy stocks, bonds, etc.)
    pub fn asset_purchase(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            event_type: EventType::AssetPurchase(AssetPurchaseSpec {
                from_account: AccountRef::Name("default".into()),
                to_asset: AssetRef::Named {
                    account: "default".into(),
                    asset: "default".into(),
                },
                amount: AmountSpec::Fixed(0.0),
            }),
            trigger: TriggerSpec::Immediate,
            once: false,
        }
    }

    /// Create a withdrawal/asset sale event
    pub fn withdrawal(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            event_type: EventType::AssetSale(AssetSaleSpec {
                to_account: AccountRef::Name("default".into()),
                amount: AmountSpec::Fixed(0.0),
                sources: WithdrawalSourceSpec::Strategy {
                    order: WithdrawalOrder::TaxEfficientEarly,
                    exclude: vec![],
                },
                amount_mode: AmountMode::Net,
                lot_method: LotMethod::Fifo,
            }),
            trigger: TriggerSpec::Immediate,
            once: false,
        }
    }

    /// Create a custom event with explicit effects
    pub fn custom(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            event_type: EventType::Custom(vec![]),
            trigger: TriggerSpec::Immediate,
            once: false,
        }
    }

    // =========================================================================
    // Account/Asset Targeting
    // =========================================================================

    /// Set the destination account by name (for income, withdrawals)
    pub fn to_account(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        match &mut self.event_type {
            EventType::Income(spec) => {
                spec.to_account = AccountRef::Name(name);
            }
            EventType::AssetSale(spec) => {
                spec.to_account = AccountRef::Name(name);
            }
            _ => {}
        }
        self
    }

    /// Set the destination account by ID
    pub fn to_account_id(mut self, id: AccountId) -> Self {
        match &mut self.event_type {
            EventType::Income(spec) => {
                spec.to_account = AccountRef::Id(id);
            }
            EventType::AssetSale(spec) => {
                spec.to_account = AccountRef::Id(id);
            }
            _ => {}
        }
        self
    }

    /// Set the source account by name (for expenses)
    pub fn from_account(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        match &mut self.event_type {
            EventType::Expense(spec) => {
                spec.from_account = AccountRef::Name(name);
            }
            EventType::AssetPurchase(spec) => {
                spec.from_account = AccountRef::Name(name);
            }
            _ => {}
        }
        self
    }

    /// Set the source account by ID
    pub fn from_account_id(mut self, id: AccountId) -> Self {
        match &mut self.event_type {
            EventType::Expense(spec) => {
                spec.from_account = AccountRef::Id(id);
            }
            EventType::AssetPurchase(spec) => {
                spec.from_account = AccountRef::Id(id);
            }
            _ => {}
        }
        self
    }

    /// Set the target asset for purchases by name
    pub fn to_asset(
        mut self,
        account_name: impl Into<String>,
        asset_name: impl Into<String>,
    ) -> Self {
        if let EventType::AssetPurchase(spec) = &mut self.event_type {
            spec.to_asset = AssetRef::Named {
                account: account_name.into(),
                asset: asset_name.into(),
            };
        }
        self
    }

    /// Set the target asset for purchases by coord
    pub fn to_asset_coord(mut self, coord: AssetCoord) -> Self {
        if let EventType::AssetPurchase(spec) = &mut self.event_type {
            spec.to_asset = AssetRef::Coord(coord);
        }
        self
    }

    // =========================================================================
    // Withdrawal Source Configuration
    // =========================================================================

    /// Withdraw from a single account
    pub fn from_single_account(mut self, account_name: impl Into<String>) -> Self {
        if let EventType::AssetSale(spec) = &mut self.event_type {
            spec.sources =
                WithdrawalSourceSpec::AccountOrder(vec![AccountRef::Name(account_name.into())]);
        }
        self
    }

    /// Withdraw from accounts in the specified order (waterfall strategy)
    pub fn from_accounts_in_order<I, S>(mut self, accounts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        if let EventType::AssetSale(spec) = &mut self.event_type {
            spec.sources = WithdrawalSourceSpec::AccountOrder(
                accounts
                    .into_iter()
                    .map(|s| AccountRef::Name(s.into()))
                    .collect(),
            );
        }
        self
    }

    /// Use a predefined withdrawal strategy
    pub fn withdrawal_strategy(mut self, order: WithdrawalOrder) -> Self {
        if let EventType::AssetSale(spec) = &mut self.event_type {
            spec.sources = WithdrawalSourceSpec::Strategy {
                order,
                exclude: vec![],
            };
        }
        self
    }

    /// Use tax-efficient withdrawal order (taxable first, then tax-deferred, then tax-free)
    pub fn tax_efficient(self) -> Self {
        self.withdrawal_strategy(WithdrawalOrder::TaxEfficientEarly)
    }

    // =========================================================================
    // Amount Configuration
    // =========================================================================

    /// Set a fixed amount
    pub fn amount(mut self, value: f64) -> Self {
        let amount = AmountSpec::Fixed(value);
        match &mut self.event_type {
            EventType::Income(spec) => spec.amount = amount,
            EventType::Expense(spec) => spec.amount = amount,
            EventType::AssetPurchase(spec) => spec.amount = amount,
            EventType::AssetSale(spec) => spec.amount = amount,
            EventType::Custom(_) => {}
        }
        self
    }

    /// Use the full source balance
    pub fn full_balance(mut self) -> Self {
        let amount = AmountSpec::SourceBalance;
        match &mut self.event_type {
            EventType::Income(spec) => spec.amount = amount,
            EventType::Expense(spec) => spec.amount = amount,
            EventType::AssetPurchase(spec) => spec.amount = amount,
            EventType::AssetSale(spec) => spec.amount = amount,
            EventType::Custom(_) => {}
        }
        self
    }

    /// Set a complex transfer amount
    pub fn transfer_amount(mut self, amount: TransferAmount) -> Self {
        let amount = AmountSpec::TransferAmount(amount);
        match &mut self.event_type {
            EventType::Income(spec) => spec.amount = amount,
            EventType::Expense(spec) => spec.amount = amount,
            EventType::AssetPurchase(spec) => spec.amount = amount,
            EventType::AssetSale(spec) => spec.amount = amount,
            EventType::Custom(_) => {}
        }
        self
    }

    // =========================================================================
    // Amount Mode (Gross vs Net)
    // =========================================================================

    /// Amount is gross (before taxes)
    pub fn gross(mut self) -> Self {
        match &mut self.event_type {
            EventType::Income(spec) => spec.amount_mode = AmountMode::Gross,
            EventType::AssetSale(spec) => spec.amount_mode = AmountMode::Gross,
            _ => {}
        }
        self
    }

    /// Amount is net (after taxes)
    pub fn net(mut self) -> Self {
        match &mut self.event_type {
            EventType::Income(spec) => spec.amount_mode = AmountMode::Net,
            EventType::AssetSale(spec) => spec.amount_mode = AmountMode::Net,
            _ => {}
        }
        self
    }

    // =========================================================================
    // Income Type
    // =========================================================================

    /// Income is taxable (default)
    pub fn taxable(mut self) -> Self {
        if let EventType::Income(spec) = &mut self.event_type {
            spec.income_type = IncomeType::Taxable;
        }
        self
    }

    /// Income is tax-free (e.g., Roth withdrawals, municipal bond interest)
    pub fn tax_free(mut self) -> Self {
        if let EventType::Income(spec) = &mut self.event_type {
            spec.income_type = IncomeType::TaxFree;
        }
        self
    }

    // =========================================================================
    // Lot Selection Method
    // =========================================================================

    /// Use FIFO (first-in, first-out) for lot selection
    pub fn fifo(mut self) -> Self {
        if let EventType::AssetSale(spec) = &mut self.event_type {
            spec.lot_method = LotMethod::Fifo;
        }
        self
    }

    /// Use LIFO (last-in, first-out) for lot selection
    pub fn lifo(mut self) -> Self {
        if let EventType::AssetSale(spec) = &mut self.event_type {
            spec.lot_method = LotMethod::Lifo;
        }
        self
    }

    /// Sell highest cost lots first (minimize gains)
    pub fn highest_cost_first(mut self) -> Self {
        if let EventType::AssetSale(spec) = &mut self.event_type {
            spec.lot_method = LotMethod::HighestCost;
        }
        self
    }

    /// Sell lowest cost lots first (realize gains)
    pub fn lowest_cost_first(mut self) -> Self {
        if let EventType::AssetSale(spec) = &mut self.event_type {
            spec.lot_method = LotMethod::LowestCost;
        }
        self
    }

    // =========================================================================
    // Timing / Triggers
    // =========================================================================

    /// Trigger on a specific date
    pub fn on_date(mut self, date: Date) -> Self {
        self.trigger = TriggerSpec::Date(date);
        self
    }

    /// Trigger at a specific age
    pub fn at_age(mut self, years: u8) -> Self {
        self.trigger = TriggerSpec::Age {
            years,
            months: None,
        };
        self
    }

    /// Trigger at a specific age and month
    pub fn at_age_months(mut self, years: u8, months: u8) -> Self {
        self.trigger = TriggerSpec::Age {
            years,
            months: Some(months),
        };
        self
    }

    /// Event triggers once (default for date/age triggers)
    pub fn once(mut self) -> Self {
        self.once = true;
        self
    }

    // =========================================================================
    // Repeating Schedules
    // =========================================================================

    /// Event repeats weekly
    pub fn weekly(mut self) -> Self {
        self.trigger = TriggerSpec::Repeating {
            interval: RepeatInterval::Weekly,
            start: self.get_start_condition(),
            end: None,
            max_occurrences: None,
        };
        self
    }

    /// Event repeats bi-weekly
    pub fn biweekly(mut self) -> Self {
        self.trigger = TriggerSpec::Repeating {
            interval: RepeatInterval::BiWeekly,
            start: self.get_start_condition(),
            end: None,
            max_occurrences: None,
        };
        self
    }

    /// Event repeats monthly
    pub fn monthly(mut self) -> Self {
        self.trigger = TriggerSpec::Repeating {
            interval: RepeatInterval::Monthly,
            start: self.get_start_condition(),
            end: None,
            max_occurrences: None,
        };
        self
    }

    /// Event repeats quarterly
    pub fn quarterly(mut self) -> Self {
        self.trigger = TriggerSpec::Repeating {
            interval: RepeatInterval::Quarterly,
            start: self.get_start_condition(),
            end: None,
            max_occurrences: None,
        };
        self
    }

    /// Event repeats yearly
    pub fn yearly(mut self) -> Self {
        self.trigger = TriggerSpec::Repeating {
            interval: RepeatInterval::Yearly,
            start: self.get_start_condition(),
            end: None,
            max_occurrences: None,
        };
        self
    }

    /// Start repeating from this date
    pub fn starting_on(mut self, date: Date) -> Self {
        if let TriggerSpec::Repeating { start, .. } = &mut self.trigger {
            *start = Some(Box::new(TriggerSpec::Date(date)));
        } else {
            // Convert to repeating with this start date
            self.trigger = TriggerSpec::Repeating {
                interval: RepeatInterval::Monthly,
                start: Some(Box::new(TriggerSpec::Date(date))),
                end: None,
                max_occurrences: None,
            };
        }
        self
    }

    /// Start repeating at this age
    pub fn starting_at_age(mut self, years: u8) -> Self {
        if let TriggerSpec::Repeating { start, .. } = &mut self.trigger {
            *start = Some(Box::new(TriggerSpec::Age {
                years,
                months: None,
            }));
        } else {
            self.trigger = TriggerSpec::Repeating {
                interval: RepeatInterval::Monthly,
                start: Some(Box::new(TriggerSpec::Age {
                    years,
                    months: None,
                })),
                end: None,
                max_occurrences: None,
            };
        }
        self
    }

    /// Stop repeating after this date
    pub fn until_date(mut self, date: Date) -> Self {
        if let TriggerSpec::Repeating { end, .. } = &mut self.trigger {
            *end = Some(Box::new(TriggerSpec::Date(date)));
        }
        self
    }

    /// Stop repeating at this age
    pub fn until_age(mut self, years: u8) -> Self {
        if let TriggerSpec::Repeating { end, .. } = &mut self.trigger {
            *end = Some(Box::new(TriggerSpec::Age {
                years,
                months: None,
            }));
        }
        self
    }

    /// Stop repeating after N occurrences
    pub fn max_occurrences(mut self, count: u32) -> Self {
        if let TriggerSpec::Repeating {
            max_occurrences, ..
        } = &mut self.trigger
        {
            *max_occurrences = Some(count);
        }
        self
    }

    // =========================================================================
    // Metadata
    // =========================================================================

    /// Set the event description
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    // =========================================================================
    // Custom Effects
    // =========================================================================

    /// Add a custom effect (for custom events)
    pub fn effect(mut self, effect: EventEffect) -> Self {
        if let EventType::Custom(effects) = &mut self.event_type {
            effects.push(effect);
        }
        self
    }

    // =========================================================================
    // Internal Helpers
    // =========================================================================

    fn get_start_condition(&self) -> Option<Box<TriggerSpec>> {
        match &self.trigger {
            TriggerSpec::Date(d) => Some(Box::new(TriggerSpec::Date(*d))),
            TriggerSpec::Age { years, months } => Some(Box::new(TriggerSpec::Age {
                years: *years,
                months: *months,
            })),
            TriggerSpec::Repeating { start, .. } => start.clone(),
            TriggerSpec::Immediate => None,
        }
    }

    // =========================================================================
    // Build
    // =========================================================================

    /// Build the event definition (to be resolved by SimulationBuilder)
    pub fn build(self) -> EventDefinition {
        EventDefinition {
            name: self.name,
            description: self.description,
            event_type: self.event_type,
            trigger: self.trigger,
            once: self.once,
        }
    }
}

/// A fully defined event ready to be added to the simulation
#[derive(Debug, Clone)]
pub struct EventDefinition {
    pub name: String,
    pub description: Option<String>,
    pub(crate) event_type: EventType,
    pub(crate) trigger: TriggerSpec,
    pub(crate) once: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_income_builder() {
        let event = EventBuilder::income("Salary")
            .to_account("Checking")
            .amount(8_000.0)
            .gross()
            .monthly()
            .until_age(65)
            .build();

        assert_eq!(event.name, "Salary");
        match event.event_type {
            EventType::Income(spec) => {
                assert!(matches!(spec.to_account, AccountRef::Name(ref n) if n == "Checking"));
                assert!(matches!(spec.amount, AmountSpec::Fixed(v) if (v - 8_000.0).abs() < 0.01));
                assert!(matches!(spec.amount_mode, AmountMode::Gross));
            }
            _ => panic!("Expected Income event type"),
        }
    }

    #[test]
    fn test_expense_builder() {
        let event = EventBuilder::expense("Rent")
            .from_account("Checking")
            .amount(2_000.0)
            .monthly()
            .build();

        assert_eq!(event.name, "Rent");
        match event.event_type {
            EventType::Expense(spec) => {
                assert!(matches!(spec.from_account, AccountRef::Name(ref n) if n == "Checking"));
            }
            _ => panic!("Expected Expense event type"),
        }
    }

    #[test]
    fn test_withdrawal_builder() {
        let event = EventBuilder::withdrawal("Retirement Income")
            .to_account("Checking")
            .amount(5_000.0)
            .net()
            .from_accounts_in_order(["Brokerage", "Traditional 401k", "Roth IRA"])
            .monthly()
            .starting_at_age(65)
            .build();

        assert_eq!(event.name, "Retirement Income");
        match event.event_type {
            EventType::AssetSale(spec) => {
                assert!(matches!(spec.amount_mode, AmountMode::Net));
                match spec.sources {
                    WithdrawalSourceSpec::AccountOrder(accounts) => {
                        assert_eq!(accounts.len(), 3);
                    }
                    _ => panic!("Expected AccountOrder sources"),
                }
            }
            _ => panic!("Expected AssetSale event type"),
        }
    }
}
