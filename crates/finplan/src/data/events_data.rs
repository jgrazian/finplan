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
    RelativeToEvent { event: EventTag, offset: OffsetData },

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
    NetWorth { threshold: ThresholdData },

    /// All conditions must be true
    And { conditions: Vec<TriggerData> },

    /// Any condition can be true
    Or { conditions: Vec<TriggerData> },

    /// Repeating schedule
    Repeating {
        interval: IntervalData,
        #[serde(skip_serializing_if = "Option::is_none")]
        start: Option<Box<TriggerData>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        end: Option<Box<TriggerData>>,
        /// Maximum number of times this event can trigger (optional)
        #[serde(skip_serializing_if = "Option::is_none")]
        max_occurrences: Option<u32>,
    },

    /// Manual trigger (only triggered by other events)
    Manual,
}

/// Transfer amount specification - supports recursive composition
///
/// This enum supports recursive amounts like `InflationAdjusted` wrapping `Scale` wrapping `Fixed`.
/// For backwards compatibility, bare floats in YAML (e.g., `amount: 5000.0`) are deserialized as `Fixed { value }`.
#[derive(Debug, Clone, PartialEq)]
pub enum AmountData {
    /// Fixed dollar amount
    Fixed { value: f64 },

    /// Inflation-adjusted wrapper - maintains purchasing power over time
    InflationAdjusted { inner: Box<AmountData> },

    /// Scale by multiplier (for percentages, e.g., 0.04 for 4%)
    Scale {
        multiplier: f64,
        inner: Box<AmountData>,
    },

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

impl AmountData {
    /// Create a fixed dollar amount
    pub fn fixed(value: f64) -> Self {
        Self::Fixed { value }
    }

    /// Wrap an amount with inflation adjustment
    pub fn inflation_adjusted(inner: AmountData) -> Self {
        Self::InflationAdjusted {
            inner: Box::new(inner),
        }
    }

    /// Create a scaled (percentage) amount
    pub fn scale(multiplier: f64, inner: AmountData) -> Self {
        Self::Scale {
            multiplier,
            inner: Box::new(inner),
        }
    }

    /// Create an inflation-adjusted fixed amount (common pattern)
    pub fn inflation_adjusted_fixed(value: f64) -> Self {
        Self::inflation_adjusted(Self::fixed(value))
    }

    /// Create a percentage of account balance (e.g., 4% withdrawal rule)
    pub fn percentage_of_account(percentage: f64, account: AccountTag) -> Self {
        Self::scale(percentage / 100.0, Self::AccountBalance { account })
    }

    /// Check if this is a simple fixed amount
    pub fn as_fixed(&self) -> Option<f64> {
        match self {
            Self::Fixed { value } => Some(*value),
            _ => None,
        }
    }

    /// Check if this amount is inflation-adjusted at the top level
    pub fn is_inflation_adjusted(&self) -> bool {
        matches!(self, Self::InflationAdjusted { .. })
    }

    /// Get the innermost amount type description
    pub fn base_type_name(&self) -> &'static str {
        match self {
            Self::Fixed { .. } => "Fixed",
            Self::InflationAdjusted { inner } => inner.base_type_name(),
            Self::Scale { inner, .. } => inner.base_type_name(),
            Self::SourceBalance => "Source Balance",
            Self::ZeroTargetBalance => "Zero Target Balance",
            Self::TargetToBalance { .. } => "Target To Balance",
            Self::AccountBalance { .. } => "Account Balance",
            Self::AccountCashBalance { .. } => "Account Cash Balance",
        }
    }
}

// Custom serialization to output as tagged enum (always use { type: ... } format)
impl Serialize for AmountData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        match self {
            Self::Fixed { value } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("type", "Fixed")?;
                map.serialize_entry("value", value)?;
                map.end()
            }
            Self::InflationAdjusted { inner } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("type", "InflationAdjusted")?;
                map.serialize_entry("inner", inner)?;
                map.end()
            }
            Self::Scale { multiplier, inner } => {
                let mut map = serializer.serialize_map(Some(3))?;
                map.serialize_entry("type", "Scale")?;
                map.serialize_entry("multiplier", multiplier)?;
                map.serialize_entry("inner", inner)?;
                map.end()
            }
            Self::SourceBalance => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("type", "SourceBalance")?;
                map.end()
            }
            Self::ZeroTargetBalance => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("type", "ZeroTargetBalance")?;
                map.end()
            }
            Self::TargetToBalance { target } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("type", "TargetToBalance")?;
                map.serialize_entry("target", target)?;
                map.end()
            }
            Self::AccountBalance { account } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("type", "AccountBalance")?;
                map.serialize_entry("account", account)?;
                map.end()
            }
            Self::AccountCashBalance { account } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("type", "AccountCashBalance")?;
                map.serialize_entry("account", account)?;
                map.end()
            }
        }
    }
}

// Custom deserialization to support both bare floats and tagged enums
impl<'de> Deserialize<'de> for AmountData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};

        struct AmountDataVisitor;

        impl<'de> Visitor<'de> for AmountDataVisitor {
            type Value = AmountData;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a number or an object with 'type' field")
            }

            // Handle bare floats for backwards compatibility
            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(AmountData::Fixed { value })
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(AmountData::Fixed {
                    value: value as f64,
                })
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(AmountData::Fixed {
                    value: value as f64,
                })
            }

            // Handle tagged objects
            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut type_str: Option<String> = None;
                let mut value: Option<f64> = None;
                let mut target: Option<f64> = None;
                let mut multiplier: Option<f64> = None;
                let mut inner: Option<AmountData> = None;
                let mut account: Option<AccountTag> = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "type" => type_str = Some(map.next_value()?),
                        "value" => value = Some(map.next_value()?),
                        "target" => target = Some(map.next_value()?),
                        "multiplier" => multiplier = Some(map.next_value()?),
                        "inner" => inner = Some(map.next_value()?),
                        "account" => account = Some(map.next_value()?),
                        _ => {
                            let _ = map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }

                let type_str = type_str.ok_or_else(|| de::Error::missing_field("type"))?;

                match type_str.as_str() {
                    "Fixed" => {
                        let value = value.ok_or_else(|| de::Error::missing_field("value"))?;
                        Ok(AmountData::Fixed { value })
                    }
                    "InflationAdjusted" => {
                        let inner = inner.ok_or_else(|| de::Error::missing_field("inner"))?;
                        Ok(AmountData::InflationAdjusted {
                            inner: Box::new(inner),
                        })
                    }
                    "Scale" => {
                        let multiplier =
                            multiplier.ok_or_else(|| de::Error::missing_field("multiplier"))?;
                        let inner = inner.ok_or_else(|| de::Error::missing_field("inner"))?;
                        Ok(AmountData::Scale {
                            multiplier,
                            inner: Box::new(inner),
                        })
                    }
                    "SourceBalance" => Ok(AmountData::SourceBalance),
                    "ZeroTargetBalance" => Ok(AmountData::ZeroTargetBalance),
                    "TargetToBalance" => {
                        let target = target.ok_or_else(|| de::Error::missing_field("target"))?;
                        Ok(AmountData::TargetToBalance { target })
                    }
                    "AccountBalance" => {
                        let account = account.ok_or_else(|| de::Error::missing_field("account"))?;
                        Ok(AmountData::AccountBalance { account })
                    }
                    "AccountCashBalance" => {
                        let account = account.ok_or_else(|| de::Error::missing_field("account"))?;
                        Ok(AmountData::AccountCashBalance { account })
                    }
                    other => Err(de::Error::unknown_variant(
                        other,
                        &[
                            "Fixed",
                            "InflationAdjusted",
                            "Scale",
                            "SourceBalance",
                            "ZeroTargetBalance",
                            "TargetToBalance",
                            "AccountBalance",
                            "AccountCashBalance",
                        ],
                    )),
                }
            }
        }

        deserializer.deserialize_any(AmountDataVisitor)
    }
}

/// Withdrawal strategy for sweep operations
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WithdrawalStrategyData {
    /// Taxable first, then tax-deferred, then tax-free
    TaxEfficient,
    /// Tax-deferred first
    TaxDeferredFirst,
    /// Tax-free first
    TaxFreeFirst,
    /// Pro-rata from all accounts
    ProRata,
    /// Penalty-aware: avoids early withdrawal penalties before age 59.5
    /// Before 59.5: Taxable → TaxFree → TaxDeferred
    /// After 59.5: Same as TaxEfficient
    #[default]
    PenaltyAware,
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
    TriggerEvent { event: EventTag },

    /// Pause a repeating event
    PauseEvent { event: EventTag },

    /// Resume a paused event
    ResumeEvent { event: EventTag },

    /// Terminate an event permanently
    TerminateEvent { event: EventTag },

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

    /// Randomly execute effects based on probability
    /// Triggers on_true event if random roll < probability, otherwise on_false
    Random {
        /// Probability threshold (0.0 to 1.0)
        probability: f64,
        /// Event to trigger on success
        on_true: EventTag,
        /// Event to trigger on failure (optional)
        #[serde(skip_serializing_if = "Option::is_none")]
        on_false: Option<EventTag>,
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
                max_occurrences: None,
            },
            effects: vec![EffectData::Income {
                to: AccountTag("Checking".to_string()),
                amount: AmountData::fixed(8000.0),
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
