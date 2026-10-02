//! `estimate_taxes(income, filing_status, state)`: one year of tax on one
//! income, through `finplan_core::taxes` — the functions the simulation itself
//! charges with — so a pay stub's net pay can be set against what the plan
//! models.
//!
//! Two figures come back. The *return estimate* is the household's tax the way
//! a return computes it: the standard deduction, the year's brackets for the
//! filing status, a state rate and payroll tax. The *plan model* is only given
//! when the plan's own tax settings are supplied: what the engine charges on
//! the same income, through its brackets and its standard deduction (before
//! any 65+ extra).

use finplan_core::model::{TaxBracket, TaxConfig};
use finplan_core::taxes::{
    calculate_federal_tax, calculate_realized_gains_tax, calculate_tax_deferred_withdrawal_tax,
};
use serde_json::{Value, json};

use super::facts::{self, DEFAULT_YEAR, FilingStatus, StateKind};

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "income": {"type": "number", "description": "Gross ordinary income for the year: wages, or other taxable income."},
            "filing_status": {"type": "string", "enum": ["single", "married_filing_jointly", "married_filing_separately", "head_of_household"], "description": "Default single."},
            "state": {"type": "string", "description": "Two-letter code or name. Omit to use the plan's state rate, or none."},
            "state_rate": {"type": "number", "description": "A fraction; overrides the state table."},
            "year": {"type": "integer", "description": "2024 to 2026 (default 2026)."},
            "pretax_contributions": {"type": "number", "description": "401(k), HSA and similar pre-tax deductions from the income (default 0)."},
            "long_term_gains": {"type": "number", "description": "Long-term capital gains on top of income (default 0), taxed at the config's flat capital gains rate."},
            "wages": {"type": "boolean", "description": "Whether the income is wages, so payroll tax applies (default true)."}
        },
        "required": ["income"]
    })
}

fn money(input: &Value, key: &str) -> Result<f64, String> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(0.0),
        Some(v) => v
            .as_f64()
            .filter(|v| v.is_finite() && *v >= 0.0)
            .ok_or_else(|| format!("`{key}` must be a number, zero or more")),
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// The engine's own configuration from a year's table.
pub fn config_for(year: i32, status: FilingStatus, state_rate: f64) -> Option<TaxConfig> {
    let brackets = facts::federal_brackets(year, status)?;
    Some(TaxConfig {
        federal_brackets: brackets
            .into_iter()
            .map(|(threshold, rate)| TaxBracket { threshold, rate })
            .collect(),
        state_rate,
        capital_gains_rate: 0.15,
        early_withdrawal_penalty_rate: 0.10,
        // `run` takes the deduction off the income itself.
        ..TaxConfig::default()
    })
}

/// `plan` is the plan's own tax configuration, when the caller has one.
pub fn run(input: &Value, plan: Option<&TaxConfig>) -> Result<Value, String> {
    let income = money(input, "income")?;
    if input.get("income").is_none() {
        return Err("income is required".into());
    }
    let pretax = money(input, "pretax_contributions")?.min(income);
    let gains = money(input, "long_term_gains")?;
    let wages = input.get("wages").and_then(Value::as_bool).unwrap_or(true);
    let year = match input.get("year") {
        None | Some(Value::Null) => DEFAULT_YEAR,
        Some(v) => v
            .as_i64()
            .and_then(|y| i32::try_from(y).ok())
            .filter(|y| facts::YEARS.contains(y))
            .ok_or("year must be 2024 to 2026")?,
    };
    let status = match input.get("filing_status").and_then(Value::as_str) {
        None => FilingStatus::Single,
        Some(s) => FilingStatus::parse(s).ok_or(
            "filing_status must be single, married_filing_jointly, married_filing_separately or head_of_household",
        )?,
    };

    let mut notes: Vec<String> = Vec::new();
    // State: an explicit rate, else the table, else the plan's.
    let (state_rate, state_source) = if let Some(rate) =
        input.get("state_rate").and_then(Value::as_f64)
    {
        if !(0.0..=1.0).contains(&rate) {
            return Err("state_rate is a fraction between 0 and 1".into());
        }
        (rate, "the state_rate given".to_owned())
    } else if let Some(query) = input.get("state").and_then(Value::as_str) {
        let (code, name, kind, rate) = facts::state(query)
            .ok_or_else(|| format!("no state matches `{query}`; use a two-letter code"))?;
        match kind {
            StateKind::None => (0.0, format!("{name} ({code}): no wage income tax")),
            StateKind::Flat => (
                rate / 100.0,
                format!("{name} ({code}): flat rate, approximate"),
            ),
            StateKind::Graduated => {
                notes.push(format!(
                    "{name}'s income tax is graduated; the top rate ({rate}%) is applied to all income, so the state figure is an upper bound."
                ));
                (
                    rate / 100.0,
                    format!("{name} ({code}): top marginal rate, approximate upper bound"),
                )
            }
        }
    } else if let Some(plan) = plan {
        (plan.state_rate, "the plan's tax settings".to_owned())
    } else {
        notes.push("No state given: state income tax is left out.".into());
        (0.0, "none given".to_owned())
    };

    let config = config_for(year, status, state_rate).ok_or("no bracket table for that year")?;
    let deduction =
        facts::standard_deduction(year, status).ok_or("no deduction table for that year")?;
    let adjusted = income - pretax;
    let taxable = (adjusted - deduction).max(0.0);
    let federal = calculate_federal_tax(taxable, &config.federal_brackets);
    let marginal = config
        .federal_brackets
        .iter()
        .rev()
        .find(|b| taxable >= b.threshold)
        .map_or(0.0, |b| b.rate);
    // The engine's flat state tax on the same base, no state deduction.
    let state_tax = adjusted * state_rate;
    let gains_tax = if gains > 0.0 {
        let g = calculate_realized_gains_tax(0.0, gains, &config, taxable);
        g.total_tax
    } else {
        0.0
    };
    let (ss_base, _) = facts::wage_base_any(year);
    let (fica_ss, fica_medicare) = if wages {
        let threshold = match status {
            FilingStatus::MarriedJoint => 250_000.0,
            FilingStatus::MarriedSeparate => 125_000.0,
            _ => 200_000.0,
        };
        (
            0.062 * income.min(ss_base),
            0.0145 * income + 0.009 * (income - threshold).max(0.0),
        )
    } else {
        (0.0, 0.0)
    };
    let total = federal + state_tax + gains_tax + fica_ss + fica_medicare;
    let gross = income + gains;

    let mut out = json!({
        "year": year,
        "filing_status": status.as_str(),
        "bracket_status": facts::bracket_status(year),
        "income": income, "pretax_contributions": pretax,
        "standard_deduction": deduction,
        "taxable_income": round2(taxable),
        "federal_income_tax": round2(federal),
        "marginal_federal_rate": marginal,
        "state_rate": state_rate, "state_source": state_source,
        "state_tax": round2(state_tax),
        "capital_gains_tax": round2(gains_tax),
        "payroll_tax": {"social_security": round2(fica_ss), "medicare": round2(fica_medicare)},
        "total_tax": round2(total),
        "net_after_tax_and_pretax": round2(gross - pretax - total),
        "effective_rate_on_gross": if gross > 0.0 { round2(total / gross * 10_000.0) / 100.0 } else { 0.0 },
        "effective_rate_unit": "percent",
    });
    notes.push("Estimate only: no credits, itemized deductions, state deductions, local taxes or phase-outs. Payroll tax is the employee share.".into());
    if let Some(plan) = plan {
        // What the engine would charge on the same ordinary income, with the
        // deduction folded into its brackets as the simulation does.
        let engine = TaxConfig {
            federal_brackets: TaxConfig::brackets_with_deduction(
                &plan.federal_brackets,
                plan.standard_deduction,
            ),
            ..plan.clone()
        };
        let charged = calculate_tax_deferred_withdrawal_tax(adjusted, &engine, 0.0);
        out["plan_model"] = json!({
            "federal_tax": round2(charged.federal_tax),
            "state_tax": round2(charged.state_tax),
            "total_tax": round2(charged.total_tax),
            "standard_deduction": plan.standard_deduction,
            "age_65_extra_deduction": plan.age_65_extra_deduction,
            "note": "What the plan's own tax settings charge on this income in the plan's first year: its brackets after its standard deduction (before any 65+ extra), a flat state rate on the whole income, and no payroll tax or credits. Later years index the brackets and deduction to inflation.",
        });
    }
    out["notes"] = json!(notes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_filer_in_2025_by_hand() {
        // $100,000 wages, single, no state, 2025: taxable 100,000 - 15,750 = 84,250.
        // 10% of 11,925 = 1,192.50; 12% of (48,475-11,925) = 4,386.00;
        // 22% of (84,250-48,475) = 7,870.50; federal 13,449.00.
        let out = run(&json!({"income": 100_000, "year": 2025}), None).unwrap();
        assert_eq!(out["taxable_income"], 84_250.0);
        assert_eq!(out["federal_income_tax"], 13_449.0);
        assert_eq!(out["marginal_federal_rate"], 0.22);
        // Payroll: 6.2% of 100,000 = 6,200; 1.45% = 1,450.
        assert_eq!(out["payroll_tax"]["social_security"], 6_200.0);
        assert_eq!(out["payroll_tax"]["medicare"], 1_450.0);
        assert_eq!(out["state_tax"], 0.0);
        assert_eq!(out["total_tax"], 21_099.0);
        assert_eq!(out["net_after_tax_and_pretax"], 78_901.0);
    }

    #[test]
    fn state_pretax_and_the_wage_base() {
        // Colorado is flat 4.4%; pre-tax 401(k) of 20,000 lowers the base.
        let out = run(
            &json!({"income": 200_000, "state": "CO", "pretax_contributions": 20_000, "filing_status": "married_filing_jointly", "year": 2025}),
            None,
        )
        .unwrap();
        assert_eq!(out["state_tax"], 7_920.0); // 180,000 * 0.044
        // Social Security stops at the 2025 wage base of 176,100: 6.2% = 10,918.20.
        assert_eq!(out["payroll_tax"]["social_security"], 10_918.2);
        let no_state = run(&json!({"income": 50_000, "state": "TX"}), None).unwrap();
        assert_eq!(no_state["state_tax"], 0.0);
        let graduated = run(&json!({"income": 50_000, "state": "CA"}), None).unwrap();
        assert!(graduated["notes"].to_string().contains("upper bound"));
    }

    #[test]
    fn the_plans_own_settings_show_what_the_engine_charges() {
        let plan = TaxConfig::default();
        // Engine, 2024 single default, $100,000: 1,160 + 4,266 + 22% of (100,000-47,150) = 17,053.00,
        // state 5% = 5,000.
        let out = run(&json!({"income": 100_000, "year": 2024}), Some(&plan)).unwrap();
        assert_eq!(out["plan_model"]["federal_tax"], 17_053.0);
        assert_eq!(out["plan_model"]["state_tax"], 5_000.0);
        assert_eq!(out["state_source"], "the plan's tax settings");
    }

    #[test]
    fn bad_input_is_refused() {
        assert!(run(&json!({}), None).is_err());
        assert!(run(&json!({"income": -1}), None).is_err());
        assert!(run(&json!({"income": 1, "filing_status": "x"}), None).is_err());
        assert!(run(&json!({"income": 1, "state": "Narnia"}), None).is_err());
        assert!(run(&json!({"income": 1, "year": 2030}), None).is_err());
    }
}
