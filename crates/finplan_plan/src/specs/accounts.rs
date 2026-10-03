//! Account specs: the flavor-specific detail, and the create/update bodies for
//! accounts and their positions (lots).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{CatchUpSpec, RepaymentSpec};
use crate::error::{PlanError, PlanResult};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum TaxStatus {
    Taxable,
    TaxDeferred,
    TaxFree,
}

impl TaxStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaxStatus::Taxable => "Taxable",
            TaxStatus::TaxDeferred => "TaxDeferred",
            TaxStatus::TaxFree => "TaxFree",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum ContributionPeriod {
    Monthly,
    Yearly,
}

impl ContributionPeriod {
    pub fn as_str(self) -> &'static str {
        match self {
            ContributionPeriod::Monthly => "Monthly",
            ContributionPeriod::Yearly => "Yearly",
        }
    }
}

/// Which retirement plan an Investment account is. It decides the tax
/// treatment and which statutory limits the web UI fills in; the engine reads
/// only the limit and catch-up figures themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum PlanType {
    /// Also 403(b), 457(b) and the TSP: they share the deferral limits.
    Traditional401k,
    Roth401k,
    TraditionalIra,
    RothIra,
    Hsa,
}

impl PlanType {
    pub fn as_str(self) -> &'static str {
        match self {
            PlanType::Traditional401k => "Traditional401k",
            PlanType::Roth401k => "Roth401k",
            PlanType::TraditionalIra => "TraditionalIra",
            PlanType::RothIra => "RothIra",
            PlanType::Hsa => "Hsa",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "Traditional401k" => PlanType::Traditional401k,
            "Roth401k" => PlanType::Roth401k,
            "TraditionalIra" => PlanType::TraditionalIra,
            "RothIra" => PlanType::RothIra,
            "Hsa" => PlanType::Hsa,
            _ => return None,
        })
    }

    /// The only tax treatment the plan can have.
    pub fn tax_status(self) -> TaxStatus {
        match self {
            PlanType::Traditional401k | PlanType::TraditionalIra => TaxStatus::TaxDeferred,
            PlanType::Roth401k | PlanType::RothIra | PlanType::Hsa => TaxStatus::TaxFree,
        }
    }
}

/// The flavor-specific half of an account.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "flavor")]
#[ts(export)]
pub enum FlavorSpec {
    Bank {
        #[serde(default)]
        cash_value: f64,
        return_profile_id: i64,
    },
    Investment {
        tax_status: TaxStatus,
        #[serde(default)]
        cash_value: f64,
        cash_return_profile_id: i64,
        #[serde(default)]
        contribution_limit: Option<f64>,
        #[serde(default)]
        contribution_period: Option<ContributionPeriod>,
        /// None for a brokerage or a retirement account of no particular plan.
        #[serde(default)]
        plan_type: Option<PlanType>,
        #[serde(default)]
        catch_up: Vec<CatchUpSpec>,
    },
    Property {
        asset_id: i64,
        #[serde(default)]
        value: f64,
    },
    Liability {
        #[serde(default)]
        principal: f64,
        #[serde(default)]
        interest_rate: f64,
        /// A fixed monthly payment that pays the loan off; absent, it is paid
        /// down only by explicit transfers.
        #[serde(default)]
        repayment: Option<RepaymentSpec>,
    },
}

impl FlavorSpec {
    pub fn name(&self) -> &'static str {
        match self {
            FlavorSpec::Bank { .. } => "Bank",
            FlavorSpec::Investment { .. } => "Investment",
            FlavorSpec::Property { .. } => "Property",
            FlavorSpec::Liability { .. } => "Liability",
        }
    }

    pub fn validate(&self) -> PlanResult<()> {
        if let FlavorSpec::Investment {
            tax_status,
            contribution_limit,
            contribution_period,
            plan_type,
            catch_up,
            ..
        } = self
        {
            if contribution_limit.is_some() != contribution_period.is_some() {
                return Err(PlanError::invalid(
                    "contribution_limit and contribution_period must be set together",
                ));
            }
            if let Some(plan) = plan_type
                && plan.tax_status().as_str() != tax_status.as_str()
            {
                return Err(PlanError::invalid(format!(
                    "a {} account is {}; set tax_status to match or change plan_type",
                    plan.as_str(),
                    plan.tax_status().as_str()
                )));
            }
            if !catch_up.is_empty() && contribution_limit.is_none() {
                return Err(PlanError::invalid(
                    "catch_up adds to a contribution_limit; set the limit first",
                ));
            }
            for tier in catch_up {
                if tier.amount.is_nan() || tier.amount < 0.0 {
                    return Err(PlanError::invalid("a catch-up amount cannot be negative"));
                }
                if tier.through_age.is_some_and(|t| t < tier.from_age) {
                    return Err(PlanError::invalid(format!(
                        "a catch-up from age {} cannot end before it starts",
                        tier.from_age
                    )));
                }
            }
        }
        if let FlavorSpec::Liability { principal, .. } = self
            && *principal < 0.0
        {
            return Err(PlanError::invalid(
                "liability principal is stored as a positive amount owed",
            ));
        }
        if let FlavorSpec::Liability {
            repayment: Some(repayment),
            ..
        } = self
            && repayment.term_months == 0
        {
            return Err(PlanError::invalid(
                "a repayment term must be at least one month",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct CreateAccount {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Omitted appends to the end of the scenario's list, which is where a new
    /// account belongs — pinning it at 0 would put it in front of every row the
    /// user has already dragged into place.
    #[serde(default)]
    pub sort_order: Option<i64>,
    #[serde(flatten)]
    pub flavor: FlavorSpec,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct UpdateAccount {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i64>,
    /// Replaces the detail row wholesale. The flavor itself cannot change:
    /// switching a 401k into a mortgage would silently invalidate every event
    /// and position pointing at it.
    ///
    /// Skipped in the TypeScript bindings: `FlavorSpec | null` has no flattened
    /// form, so the web client composes the union itself as
    /// `UpdateAccount & (FlavorSpec | {})` in `web/lib/api/types.ts`.
    #[serde(default, flatten)]
    #[ts(skip)]
    pub flavor: Option<FlavorSpec>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct CreatePosition {
    pub asset_id: i64,
    /// Defaults to the scenario start date, i.e. an opening holding.
    #[serde(default)]
    pub purchase_date: Option<String>,
    pub units: f64,
    pub cost_basis: f64,
}

/// Every field of a lot is resizable in place. Absent means unchanged, so a
/// client that only wants to resize a holding sends `units` alone.
#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct UpdatePosition {
    #[serde(default)]
    pub asset_id: Option<i64>,
    #[serde(default)]
    pub purchase_date: Option<String>,
    #[serde(default)]
    pub units: Option<f64>,
    #[serde(default)]
    pub cost_basis: Option<f64>,
}
