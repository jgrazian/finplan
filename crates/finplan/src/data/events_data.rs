use serde::{Deserialize, Serialize};

use super::portfolio_data::AssetTag;

/// String-based event reference for human-readable YAML
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct EventTag(pub String);

/// String-based account reference for human-readable YAML
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AccountTag(pub String);

/// Time offset relative to another event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "unit")]
pub enum OffsetData {
    Days { value: i32 },
    Months { value: i32 },
    Years { value: i32 },
}

/// Balance threshold condition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "comparison")]
pub enum ThresholdData {
    GreaterThanOrEqual { value: f64 },
    LessThanOrEqual { value: f64 },
}

/// Repeat interval for recurring events
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum IntervalData {
    Never,
    Weekly,
    BiWeekly,
    #[default]
    Monthly,
    Quarterly,
    Yearly,
}

/// Human-readable event trigger using string names
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TriggerData {
    /// Trigger on a specific date
    Date {
        date: String, // "2025-01-01" format
    },

    /// Trigger at a specific age
    Age {
        years: u8,
        #[serde(skip_serializing_if = "Option::is_none")]
        months: Option<u8>,
    },

    /// Trigger relative to another event
    RelativeToEvent {
        event: EventTag,
        offset: OffsetData,
    },

    /// Trigger when account balance crosses threshold
    AccountBalance {
        account: AccountTag,
        threshold: ThresholdData,
    },

    /// Trigger when asset balance crosses threshold
    AssetBalance {
        account: AccountTag,
        asset: AssetTag,
        threshold: ThresholdData,
    },

    /// Trigger when total net worth crosses threshold
    NetWorth {
        threshold: ThresholdData,
    },

    /// All conditions must be true
    And {
        conditions: Vec<TriggerData>,
    },

    /// Any condition can be true
    Or {
        conditions: Vec<TriggerData>,
    },

    /// Repeating schedule
    Repeating {
        interval: IntervalData,
        #[serde(skip_serializing_if = "Option::is_none")]
        start: Option<Box<TriggerData>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        end: Option<Box<TriggerData>>,
    },

    /// Manual trigger (only triggered by other events)
    Manual,
}

/// Transfer amount specification
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AmountData {
    /// Fixed dollar amount
    Fixed(f64),
    /// Special amount calculation
    Special(SpecialAmount),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SpecialAmount {
    /// Transfer entire source balance
    SourceBalance,
    /// Transfer enough to zero out target balance (debt payoff)
    ZeroTargetBalance,
    /// Transfer enough to bring target to specified balance
    TargetToBalance { target: f64 },
    /// Reference account's total balance
    AccountBalance { account: AccountTag },
    /// Reference account's cash balance
    AccountCashBalance { account: AccountTag },
}

/// Withdrawal strategy for sweep operations
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WithdrawalStrategyData {
    /// Taxable first, then tax-deferred, then tax-free
    #[default]
    TaxEfficient,
    /// Tax-deferred first
    TaxDeferredFirst,
    /// Tax-free first
    TaxFreeFirst,
    /// Pro-rata from all accounts
    ProRata,
}

/// Lot selection method for asset sales
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LotMethodData {
    #[default]
    Fifo,
    Lifo,
    HighestCost,
    LowestCost,
    AverageCost,
}

/// Human-readable event effect using string names
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EffectData {
    /// Receive income into an account
    Income {
        to: AccountTag,
        amount: AmountData,
        #[serde(default)]
        gross: bool,
        #[serde(default = "default_true")]
        taxable: bool,
    },

    /// Pay an expense from an account
    Expense {
        from: AccountTag,
        amount: AmountData,
    },

    /// Purchase assets with cash
    AssetPurchase {
        from: AccountTag,
        to_account: AccountTag,
        asset: AssetTag,
        amount: AmountData,
    },

    /// Sell assets from an account
    AssetSale {
        from: AccountTag,
        #[serde(skip_serializing_if = "Option::is_none")]
        asset: Option<AssetTag>,
        amount: AmountData,
        #[serde(default)]
        gross: bool,
        #[serde(default)]
        lot_method: LotMethodData,
    },

    /// Sweep: liquidate and transfer to another account
    Sweep {
        to: AccountTag,
        amount: AmountData,
        #[serde(default)]
        strategy: WithdrawalStrategyData,
        #[serde(default)]
        gross: bool,
        #[serde(default = "default_true")]
        taxable: bool,
        #[serde(default)]
        lot_method: LotMethodData,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        exclude_accounts: Vec<AccountTag>,
    },

    /// Trigger another event
    TriggerEvent {
        event: EventTag,
    },

    /// Pause a repeating event
    PauseEvent {
        event: EventTag,
    },

    /// Resume a paused event
    ResumeEvent {
        event: EventTag,
    },

    /// Terminate an event permanently
    TerminateEvent {
        event: EventTag,
    },

    /// Apply Required Minimum Distributions
    ApplyRmd {
        destination: AccountTag,
        #[serde(default)]
        lot_method: LotMethodData,
    },

    /// Directly adjust an account's balance
    /// For liabilities: positive = increase debt, negative = decrease debt
    /// For cash accounts: positive = add cash, negative = remove cash
    AdjustBalance {
        account: AccountTag,
        amount: AmountData,
    },

    /// Transfer cash between accounts
    /// If destination is a liability, reduces the principal
    CashTransfer {
        from: AccountTag,
        to: AccountTag,
        amount: AmountData,
    },
}

fn default_true() -> bool {
    true
}

/// Complete event definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventData {
    pub name: EventTag,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub trigger: TriggerData,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<EffectData>,
    #[serde(default)]
    pub once: bool,
    /// Whether this event is enabled in simulation (default true)
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization() {
        let event = EventData {
            name: EventTag("Monthly Salary".to_string()),
            description: Some("Regular monthly income".to_string()),
            trigger: TriggerData::Repeating {
                interval: IntervalData::Monthly,
                start: None,
                end: Some(Box::new(TriggerData::Age {
                    years: 65,
                    months: None,
                })),
            },
            effects: vec![EffectData::Income {
                to: AccountTag("Checking".to_string()),
                amount: AmountData::Fixed(8000.0),
                gross: true,
                taxable: true,
            }],
            once: false,
            enabled: true,
        };

        let yaml = serde_saphyr::to_string(&event).unwrap();
        println!("Event YAML:\n{}", yaml);

        let deserialized: EventData = serde_saphyr::from_str(&yaml).unwrap();
        assert_eq!(deserialized.name.0, "Monthly Salary");
    }

    #[test]
    fn test_complex_trigger() {
        let trigger = TriggerData::And {
            conditions: vec![
                TriggerData::Age {
                    years: 65,
                    months: None,
                },
                TriggerData::AccountBalance {
                    account: AccountTag("401k".to_string()),
                    threshold: ThresholdData::GreaterThanOrEqual { value: 100000.0 },
                },
            ],
        };

        let yaml = serde_saphyr::to_string(&trigger).unwrap();
        println!("Complex trigger YAML:\n{}", yaml);

        let deserialized: TriggerData = serde_saphyr::from_str(&yaml).unwrap();
        match deserialized {
            TriggerData::And { conditions } => assert_eq!(conditions.len(), 2),
            _ => panic!("Expected And trigger"),
        }
    }
}
