//! The `Spending` effect's rule and portfolio, as the API shows them (spec 22).
//!
//! The rule holds no ids, so it is stored whole, as JSON, in the effect row's
//! `spending_rule` column; the portfolio's accounts are withdrawal items with
//! the `portfolio` role, so duplication, archives and account deletion treat
//! them like any other account reference.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{PlanError, PlanResult};

/// How a rule sets its first year's draw.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum SpendingInitialSpec {
    /// A yearly amount in today's (plan-start) dollars.
    Amount { amount: f64 },
    /// A share of the portfolio when spending starts.
    Rate { rate: f64 },
}

fn default_band() -> f64 {
    0.20
}
fn default_step() -> f64 {
    0.10
}
fn default_true() -> bool {
    true
}
fn default_protect() -> u8 {
    15
}
fn default_max_cut() -> f64 {
    0.025
}
fn default_max_raise() -> f64 {
    0.05
}
fn default_final_age() -> u8 {
    100
}

/// How a `Spending` effect decides each year's draw. Dollar figures (an
/// initial amount, `floor`, `ceiling`) are in today's dollars; rates are
/// fractions of the portfolio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "rule")]
#[ts(export)]
pub enum SpendingRuleSpec {
    /// `rate` × the portfolio, between `floor` and `ceiling`.
    PercentOfPortfolio {
        rate: f64,
        #[serde(default)]
        floor: Option<f64>,
        #[serde(default)]
        ceiling: Option<f64>,
    },
    /// Guyton-Klinger guardrails: last year's draw, raised for inflation, cut
    /// by `step` when the withdrawal rate rises `band` above where it started
    /// and raised by `step` when it falls `band` below.
    Guardrails {
        initial: SpendingInitialSpec,
        #[serde(default = "default_band")]
        band: f64,
        #[serde(default = "default_step")]
        step: f64,
        #[serde(default = "default_true")]
        skip_inflation_after_loss: bool,
        #[serde(default = "default_protect")]
        protect_final_years: u8,
        #[serde(default)]
        floor: Option<f64>,
        #[serde(default)]
        ceiling: Option<f64>,
    },
    /// Vanguard-style: toward `rate` × the portfolio, moving at most
    /// `max_cut` down or `max_raise` up a year after inflation.
    CappedChange {
        rate: f64,
        #[serde(default = "default_max_cut")]
        max_cut: f64,
        #[serde(default = "default_max_raise")]
        max_raise: f64,
    },
    /// Variable percentage withdrawal: spend the portfolio down by
    /// `final_age`. `real_return` (after inflation) defaults to the plan's
    /// own return and inflation assumptions for the portfolio.
    Vpw {
        #[serde(default)]
        real_return: Option<f64>,
        #[serde(default = "default_final_age")]
        final_age: u8,
    },
}

/// The accounts a rule reads its balance from.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum SpendingPortfolioSpec {
    /// Every bank and investment account.
    #[default]
    Liquid,
    /// Exactly these bank or investment accounts.
    Accounts { account_ids: Vec<i64> },
}

impl SpendingInitialSpec {
    fn validate(&self) -> PlanResult<()> {
        match self {
            SpendingInitialSpec::Amount { amount } => {
                if !(amount.is_finite() && *amount > 0.0) {
                    return Err(PlanError::invalid(
                        "a starting amount must be more than zero",
                    ));
                }
            }
            SpendingInitialSpec::Rate { rate } => check_rate(*rate)?,
        }
        Ok(())
    }
}

fn check_rate(rate: f64) -> PlanResult<()> {
    if rate.is_finite() && rate > 0.0 && rate <= 0.2 {
        Ok(())
    } else {
        Err(PlanError::invalid(
            "a withdrawal rate is a fraction above 0 and at most 0.2 (20%)",
        ))
    }
}

fn check_fraction(value: f64, what: &str) -> PlanResult<()> {
    if value.is_finite() && value > 0.0 && value < 1.0 {
        Ok(())
    } else {
        Err(PlanError::invalid(format!(
            "{what} is a fraction between 0 and 1"
        )))
    }
}

fn check_clamps(floor: Option<f64>, ceiling: Option<f64>) -> PlanResult<()> {
    for value in [floor, ceiling].into_iter().flatten() {
        if !(value.is_finite() && value >= 0.0) {
            return Err(PlanError::invalid(
                "a floor or ceiling is an amount of zero or more",
            ));
        }
    }
    if let (Some(floor), Some(ceiling)) = (floor, ceiling)
        && floor > ceiling
    {
        return Err(PlanError::invalid("the floor is above the ceiling"));
    }
    Ok(())
}

impl SpendingRuleSpec {
    /// The checks that need nothing but the rule itself. The ones that need
    /// the plan (the final age against the owner's, the event's interval,
    /// the accounts) run in `compile::check_spending`.
    pub fn validate(&self) -> PlanResult<()> {
        match self {
            SpendingRuleSpec::PercentOfPortfolio {
                rate,
                floor,
                ceiling,
            } => {
                check_rate(*rate)?;
                check_clamps(*floor, *ceiling)
            }
            SpendingRuleSpec::Guardrails {
                initial,
                band,
                step,
                floor,
                ceiling,
                ..
            } => {
                initial.validate()?;
                check_fraction(*band, "the guardrail band")?;
                check_fraction(*step, "the guardrail step")?;
                check_clamps(*floor, *ceiling)
            }
            SpendingRuleSpec::CappedChange {
                rate,
                max_cut,
                max_raise,
            } => {
                check_rate(*rate)?;
                if !(max_cut.is_finite() && (0.0..1.0).contains(max_cut)) {
                    return Err(PlanError::invalid(
                        "the largest cut is a fraction from 0 up to 1",
                    ));
                }
                if !(max_raise.is_finite() && (0.0..=1.0).contains(max_raise)) {
                    return Err(PlanError::invalid(
                        "the largest raise is a fraction from 0 to 1",
                    ));
                }
                Ok(())
            }
            SpendingRuleSpec::Vpw { real_return, .. } => match real_return {
                Some(r) if !(r.is_finite() && (-0.5..=0.5).contains(r)) => Err(PlanError::invalid(
                    "a real return is a fraction between -0.5 and 0.5",
                )),
                _ => Ok(()),
            },
        }
    }

    /// The rule's name as the screens show it.
    pub fn label(&self) -> &'static str {
        match self {
            SpendingRuleSpec::PercentOfPortfolio { .. } => "Percent of portfolio",
            SpendingRuleSpec::Guardrails { .. } => "Guardrails",
            SpendingRuleSpec::CappedChange { .. } => "Capped change",
            SpendingRuleSpec::Vpw { .. } => "VPW",
        }
    }

    /// Stored form: the rule as JSON.
    pub fn to_column(&self) -> String {
        serde_json::to_string(self).expect("a spending rule always serializes")
    }

    /// Read the stored form back.
    pub fn from_column(column: &str) -> PlanResult<Self> {
        serde_json::from_str(column)
            .map_err(|e| PlanError::internal(format!("stored spending rule is unreadable: {e}")))
    }

    /// The rule for the engine. `real_return` is what a VPW rule without its
    /// own uses (the plan's expected real return on the portfolio).
    pub fn to_core(&self, real_return: f64) -> finplan_core::model::SpendingRule {
        use finplan_core::model::{SpendingInitial, SpendingRule};
        match self {
            SpendingRuleSpec::PercentOfPortfolio {
                rate,
                floor,
                ceiling,
            } => SpendingRule::PercentOfPortfolio {
                rate: *rate,
                floor: *floor,
                ceiling: *ceiling,
            },
            SpendingRuleSpec::Guardrails {
                initial,
                band,
                step,
                skip_inflation_after_loss,
                protect_final_years,
                floor,
                ceiling,
            } => SpendingRule::Guardrails {
                initial: match initial {
                    SpendingInitialSpec::Amount { amount } => SpendingInitial::Amount(*amount),
                    SpendingInitialSpec::Rate { rate } => SpendingInitial::Rate(*rate),
                },
                band: *band,
                step: *step,
                skip_inflation_after_loss: *skip_inflation_after_loss,
                protect_final_years: *protect_final_years,
                floor: *floor,
                ceiling: *ceiling,
            },
            SpendingRuleSpec::CappedChange {
                rate,
                max_cut,
                max_raise,
            } => SpendingRule::CappedChange {
                rate: *rate,
                max_cut: *max_cut,
                max_raise: *max_raise,
            },
            SpendingRuleSpec::Vpw {
                real_return: own,
                final_age,
            } => SpendingRule::Vpw {
                real_return: own.unwrap_or(real_return),
                final_age: *final_age,
            },
        }
    }

    /// The first-year draw this rule names in today's dollars, when it names
    /// one (an initial amount); a rate-based rule depends on the portfolio.
    pub fn initial_amount(&self) -> Option<f64> {
        match self {
            SpendingRuleSpec::Guardrails {
                initial: SpendingInitialSpec::Amount { amount },
                ..
            } => Some(*amount),
            _ => None,
        }
    }

    /// The same rule starting at `amount` a year in today's dollars, where
    /// the rule has a starting amount (Guardrails), or at the rate `amount`
    /// is of `portfolio` otherwise.
    pub fn with_starting_amount(&self, amount: f64, portfolio: f64) -> Self {
        let rate = if portfolio > 0.0 {
            (amount / portfolio).clamp(0.001, 0.2)
        } else {
            0.04
        };
        match self.clone() {
            SpendingRuleSpec::Guardrails {
                band,
                step,
                skip_inflation_after_loss,
                protect_final_years,
                floor,
                ceiling,
                ..
            } => SpendingRuleSpec::Guardrails {
                initial: SpendingInitialSpec::Amount { amount },
                band,
                step,
                skip_inflation_after_loss,
                protect_final_years,
                floor,
                ceiling,
            },
            SpendingRuleSpec::PercentOfPortfolio { floor, ceiling, .. } => {
                SpendingRuleSpec::PercentOfPortfolio {
                    rate,
                    floor,
                    ceiling,
                }
            }
            SpendingRuleSpec::CappedChange {
                max_cut, max_raise, ..
            } => SpendingRuleSpec::CappedChange {
                rate,
                max_cut,
                max_raise,
            },
            vpw @ SpendingRuleSpec::Vpw { .. } => vpw,
        }
    }

    /// Guardrails at their published defaults, starting at `amount`.
    pub fn guardrails(amount: f64) -> Self {
        SpendingRuleSpec::Guardrails {
            initial: SpendingInitialSpec::Amount { amount },
            band: default_band(),
            step: default_step(),
            skip_inflation_after_loss: true,
            protect_final_years: default_protect(),
            floor: None,
            ceiling: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rule_round_trips_through_its_column() {
        let rule = SpendingRuleSpec::guardrails(80_000.0);
        assert_eq!(
            SpendingRuleSpec::from_column(&rule.to_column()).unwrap(),
            rule
        );
    }

    #[test]
    fn omitted_fields_take_the_published_defaults() {
        let rule: SpendingRuleSpec = serde_json::from_value(serde_json::json!({
            "rule": "Guardrails",
            "initial": { "kind": "Amount", "amount": 60000.0 }
        }))
        .unwrap();
        assert_eq!(rule, SpendingRuleSpec::guardrails(60_000.0));
        let vpw: SpendingRuleSpec =
            serde_json::from_value(serde_json::json!({ "rule": "Vpw" })).unwrap();
        assert_eq!(
            vpw,
            SpendingRuleSpec::Vpw {
                real_return: None,
                final_age: 100
            }
        );
    }

    #[test]
    fn validation_holds_the_ranges() {
        let percent = |rate, floor, ceiling| SpendingRuleSpec::PercentOfPortfolio {
            rate,
            floor,
            ceiling,
        };
        assert!(percent(0.04, None, None).validate().is_ok());
        assert!(percent(0.0, None, None).validate().is_err());
        assert!(percent(0.25, None, None).validate().is_err());
        assert!(percent(0.04, Some(50.0), Some(40.0)).validate().is_err());
        assert!(percent(0.04, Some(40.0), Some(50.0)).validate().is_ok());
        let guardrails = |band, step| SpendingRuleSpec::Guardrails {
            initial: SpendingInitialSpec::Amount { amount: 1.0 },
            band,
            step,
            skip_inflation_after_loss: true,
            protect_final_years: 15,
            floor: None,
            ceiling: None,
        };
        assert!(guardrails(0.2, 0.1).validate().is_ok());
        assert!(guardrails(1.0, 0.1).validate().is_err());
        assert!(guardrails(0.2, 0.0).validate().is_err());
    }
}
