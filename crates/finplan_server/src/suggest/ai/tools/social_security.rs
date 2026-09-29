//! `estimate_social_security(earnings, birth_year, claim_age)`: a retirement
//! benefit from the statutory formula, the same answer every time.
//!
//! Two ways in. With an earnings history, each year's covered earnings (capped
//! at that year's wage base) are indexed to the year the person turns 60 by the
//! national average wage index, the highest 35 are averaged (AIME), and the
//! benefit at full retirement age (PIA) follows from the bend points for the
//! year they turn 62. With only a current salary, the method is stated and
//! simple: earnings in wage-indexed (today's) dollars are the salary capped at
//! this year's wage base for `career_years` of the 35 counted years, and the
//! bend points are this year's, so the result is in today's dollars. Claiming
//! before or after full retirement age then reduces or raises the PIA by the
//! statutory monthly factors. Cost-of-living adjustments, spousal and survivor
//! benefits, the earnings test and taxation of benefits are not modeled.

use serde_json::{Value, json};

use super::facts::{self, DEFAULT_YEAR};

/// Years counted in AIME.
const COMPUTATION_YEARS: usize = 35;

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "birth_year": {"type": "integer", "description": "Four digits; 1943 or later."},
            "claim_age": {"type": "number", "description": "Age benefits start, 62 to 70, in years and fractions (67, 62.5). Default: full retirement age."},
            "earnings": {
                "type": "array",
                "description": "Covered (Social Security taxed) earnings by calendar year, 1980 on; nominal dollars, uncapped is fine (capped at each year's wage base). Years not listed count as zero. May include future years.",
                "items": {"type": "object", "properties": {"year": {"type": "integer"}, "amount": {"type": "number"}}, "required": ["year", "amount"]},
                "maxItems": 80
            },
            "current_salary": {"type": "number", "description": "When there is no earnings history: today's annual earnings. The estimate then assumes that level, in today's dollars, for career_years of the 35 counted years."},
            "career_years": {"type": "integer", "description": "With current_salary: years of covered earnings over a whole career, 1 to 35 (default 35)."},
            "as_of_year": {"type": "integer", "description": "With current_salary: the year 'today's dollars' means; 2024 to 2026 (default 2026)."}
        },
        "required": ["birth_year"]
    })
}

/// One year of covered earnings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Earning {
    pub year: i32,
    pub amount: f64,
}

/// The primary insurance amount for an AIME: 90% up to the first bend point,
/// 32% up to the second, 15% above, rounded down to the dime.
pub fn pia(aime: f64, (first, second): (f64, f64)) -> f64 {
    let aime = aime.max(0.0);
    let raw = 0.9 * aime.min(first)
        + 0.32 * (aime.min(second) - first).max(0.0)
        + 0.15 * (aime - second).max(0.0);
    (raw * 10.0 + 1e-9).floor() / 10.0
}

/// Months from the FRA to `claim_months` as a benefit factor: reduced 5/9 of
/// 1% a month for the first 36 months early and 5/12 of 1% beyond, raised 2/3
/// of 1% a month late (to 70).
pub fn claiming_factor(fra_months: u32, claim_months: u32) -> f64 {
    if claim_months < fra_months {
        let early = f64::from(fra_months - claim_months);
        let first = early.min(36.0);
        let rest = early - first;
        1.0 - first * 5.0 / 900.0 - rest * 5.0 / 1200.0
    } else {
        let late = f64::from(claim_months.min(70 * 12) - fra_months);
        1.0 + late * 2.0 / 300.0
    }
}

/// A monthly benefit rounded down to the dollar, as SSA pays it.
fn benefit(pia: f64, factor: f64) -> f64 {
    (pia * factor + 1e-9).floor()
}

/// What an earnings history came to.
#[derive(Debug, Clone, PartialEq)]
pub struct Aime {
    pub aime: f64,
    /// Wage-indexed earnings of the counted years, highest first.
    pub top_years: Vec<(i32, f64)>,
    /// Some index or wage base used was projected, not published.
    pub projected: bool,
    /// Years whose earnings were capped at the wage base.
    pub capped_years: Vec<i32>,
}

/// AIME from a history, for someone born in `birth_year`.
pub fn aime_from_earnings(birth_year: i32, earnings: &[Earning]) -> Result<Aime, String> {
    let indexing_year = birth_year + 60;
    let (index, mut projected) = facts::awi(indexing_year);
    let mut capped_years = Vec::new();
    let mut indexed: Vec<(i32, f64)> = Vec::new();
    for e in earnings {
        if !facts::awi_covers(e.year) {
            return Err(format!(
                "earnings from {} are before 1980, which the wage table does not cover; leave them out",
                e.year
            ));
        }
        if e.amount < 0.0 || !e.amount.is_finite() {
            return Err(format!("earnings for {} must be zero or more", e.year));
        }
        let (base, base_projected) = facts::wage_base_any(e.year);
        projected |= base_projected;
        let covered = if e.amount > base {
            capped_years.push(e.year);
            base
        } else {
            e.amount
        };
        let value = if e.year < indexing_year {
            let (year_index, p) = facts::awi(e.year);
            projected |= p;
            covered * index / year_index
        } else {
            covered
        };
        indexed.push((e.year, value));
    }
    let mut years: Vec<i32> = indexed.iter().map(|(y, _)| *y).collect();
    years.sort_unstable();
    years.dedup();
    if years.len() != indexed.len() {
        return Err("each year may appear once in earnings".into());
    }
    indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    indexed.truncate(COMPUTATION_YEARS);
    let total: f64 = indexed.iter().map(|(_, v)| v).sum();
    Ok(Aime {
        aime: (total / (COMPUTATION_YEARS as f64 * 12.0)).floor(),
        top_years: indexed,
        projected,
        capped_years,
    })
}

/// AIME for a flat salary in today's dollars over `career_years` of the 35.
pub fn aime_from_salary(salary: f64, career_years: u32, as_of_year: i32) -> (f64, f64) {
    let base = facts::wage_base(as_of_year).unwrap_or(0.0);
    let covered = salary.min(base);
    let counted = f64::from(career_years.min(COMPUTATION_YEARS as u32));
    (
        (covered * counted / (COMPUTATION_YEARS as f64 * 12.0)).floor(),
        covered,
    )
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

pub fn run(input: &Value) -> Result<Value, String> {
    let birth_year = input
        .get("birth_year")
        .and_then(Value::as_i64)
        .and_then(|y| i32::try_from(y).ok())
        .filter(|y| (1943..=2010).contains(y))
        .ok_or("birth_year is required, from 1943 to 2010")?;
    let (fra_y, fra_m) = facts::full_retirement_age(birth_year);
    let fra_months = fra_y * 12 + fra_m;
    let claim_months = match input.get("claim_age") {
        None | Some(Value::Null) => fra_months,
        Some(v) => {
            let age = v
                .as_f64()
                .filter(|a| (62.0..=70.0).contains(a))
                .ok_or("claim_age must be from 62 to 70")?;
            (age * 12.0).round() as u32
        }
    };

    let earnings: Option<Vec<Earning>> = match input.get("earnings") {
        None | Some(Value::Null) => None,
        Some(Value::Array(items)) if items.is_empty() => None,
        Some(Value::Array(items)) => Some(
            items
                .iter()
                .map(|item| {
                    let year = item
                        .get("year")
                        .and_then(Value::as_i64)
                        .and_then(|y| i32::try_from(y).ok())
                        .ok_or("each earnings entry needs an integer year")?;
                    let amount = item
                        .get("amount")
                        .and_then(Value::as_f64)
                        .ok_or("each earnings entry needs a numeric amount")?;
                    Ok(Earning { year, amount })
                })
                .collect::<Result<_, &str>>()?,
        ),
        Some(_) => return Err("earnings must be a list of {year, amount}".into()),
    };
    let salary = input.get("current_salary").and_then(Value::as_f64);

    let (aime, bends, bends_projected, method, mut notes, mut extra) = match (&earnings, salary) {
        (Some(history), _) => {
            let calc = aime_from_earnings(birth_year, history)?;
            let eligible = birth_year + 62;
            let (bends, bp) = facts::bend_points(eligible);
            let mut notes = vec![format!(
                "Earnings before age 60 (before {}) are indexed to {} by the national average wage index; later years count at face value; the highest {COMPUTATION_YEARS} years are averaged over {} months.",
                birth_year + 60,
                birth_year + 60,
                COMPUTATION_YEARS * 12
            )];
            if calc.top_years.len() < COMPUTATION_YEARS {
                notes.push(format!(
                    "Only {} earnings years given; the rest of the {COMPUTATION_YEARS} count as zero, which lowers the result.",
                    calc.top_years.len()
                ));
            }
            if !calc.capped_years.is_empty() {
                notes.push(format!(
                    "Earnings above the wage base were capped in {} years.",
                    calc.capped_years.len()
                ));
            }
            if calc.projected || bp {
                notes.push("Wage indexes or wage bases after the last published year are projected at 3.5% growth a year.".into());
            }
            let extra = json!({
                "counted_years": calc.top_years.len(),
                "top_indexed_years": calc.top_years.iter().take(5).map(|(y, v)| json!({"year": y, "indexed": round2(*v)})).collect::<Vec<_>>(),
                "dollars": format!("dollars of {eligible}, the year of eligibility at 62 (wage-indexed; benefits then rise with cost-of-living adjustments)"),
            });
            (
                calc.aime,
                bends,
                bp || calc.projected,
                format!(
                    "earnings history: wage-indexed to {}, top {COMPUTATION_YEARS} years",
                    birth_year + 60
                ),
                notes,
                extra,
            )
        }
        (None, Some(salary)) => {
            if !salary.is_finite() || salary < 0.0 {
                return Err("current_salary must be zero or more".into());
            }
            let career = match input.get("career_years") {
                None | Some(Value::Null) => COMPUTATION_YEARS as u32,
                Some(v) => v
                    .as_u64()
                    .filter(|c| (1..=COMPUTATION_YEARS as u64).contains(c))
                    .ok_or("career_years must be 1 to 35")? as u32,
            };
            let as_of = match input.get("as_of_year") {
                None | Some(Value::Null) => DEFAULT_YEAR,
                Some(v) => v
                    .as_i64()
                    .and_then(|y| i32::try_from(y).ok())
                    .filter(|y| facts::YEARS.contains(y))
                    .ok_or("as_of_year must be 2024 to 2026")?,
            };
            let (aime, covered) = aime_from_salary(salary, career, as_of);
            let (bends, bp) = facts::bend_points(as_of);
            let mut notes = vec![format!(
                "Method: earnings in {as_of} dollars equal to the salary, capped at the {as_of} wage base (${}), for {career} of the {COMPUTATION_YEARS} counted years; bend points for {as_of}. The result is in {as_of} dollars.",
                facts::wage_base(as_of).unwrap_or(0.0)
            )];
            if salary > covered {
                notes
                    .push("The salary is above the wage base; the excess earns no benefit.".into());
            }
            notes.push(
                "The person's own SSA statement (ssa.gov/myaccount) replaces this estimate.".into(),
            );
            let extra = json!({
                "counted_years": career, "covered_earnings": covered,
                "dollars": format!("{as_of} dollars"),
            });
            (
                aime,
                bends,
                bp,
                format!("flat salary in today's dollars, {career} of {COMPUTATION_YEARS} years"),
                notes,
                extra,
            )
        }
        (None, None) => return Err("give earnings (a history) or current_salary".into()),
    };

    let base_pia = pia(aime, bends);
    let factor = claiming_factor(fra_months, claim_months);
    let monthly = benefit(base_pia, factor);
    let at = |months: u32| benefit(base_pia, claiming_factor(fra_months, months));
    notes.push("Not modeled: cost-of-living adjustments, spousal and survivor benefits, the earnings test before full retirement age, and federal taxation of benefits.".into());
    extra["method"] = json!(method);
    Ok(json!({
        "aime": aime,
        "pia": base_pia,
        "bend_points": {"first": bends.0, "second": bends.1, "projected": bends_projected},
        "full_retirement_age": {"years": fra_y, "months": fra_m},
        "claim_age": f64::from(claim_months) / 12.0,
        "claiming_factor": (factor * 10_000.0).round() / 10_000.0,
        "monthly_benefit": monthly,
        "annual_benefit": monthly * 12.0,
        "monthly_benefit_by_claim_age": {
            "62": at(62 * 12), "full_retirement_age": at(fra_months), "70": at(70 * 12),
        },
        "details": extra,
        "notes": notes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pia_uses_the_bend_points() {
        // 2026 bend points: 0.9*1,286 + 0.32*(5,000-1,286) = 1,157.40 + 1,188.48 = 2,345.88 -> 2,345.80.
        assert_eq!(pia(5_000.0, (1_286.0, 7_749.0)), 2_345.8);
        // Below the first bend point: 90%.
        assert_eq!(pia(1_000.0, (1_286.0, 7_749.0)), 900.0);
        // Above the second: 1,157.40 + 0.32*6,463 + 0.15*(10,000-7,749) = 3,563.21 -> 3,563.20.
        assert_eq!(pia(10_000.0, (1_286.0, 7_749.0)), 3_563.2);
    }

    #[test]
    fn claiming_adjustments() {
        // FRA 67 (804 months). Claim at 62: 60 months early = 36*5/9% + 24*5/12% = 20% + 10%.
        assert!((claiming_factor(804, 744) - 0.70).abs() < 1e-12);
        // Claim at 66: 12 months early = 6.67%.
        assert!((claiming_factor(804, 792) - (1.0 - 12.0 * 5.0 / 900.0)).abs() < 1e-12);
        // Claim at 70: 36 months late = 24%.
        assert!((claiming_factor(804, 840) - 1.24).abs() < 1e-12);
        // No credit past 70.
        assert!((claiming_factor(804, 900) - 1.24).abs() < 1e-12);
        // FRA 66: claiming at 62 is 48 months early: 20% + 12*5/12% = 25%.
        assert!((claiming_factor(792, 744) - 0.75).abs() < 1e-12);
    }

    #[test]
    fn a_flat_salary_projection_by_hand() {
        // $100,000 for 35 years: AIME floor(100,000/12) = 8,333.
        // PIA = 1,157.40 + 0.32*6,463 + 0.15*584 = 3,313.16 -> 3,313.10.
        let out =
            run(&json!({"birth_year": 1965, "claim_age": 67, "current_salary": 100_000})).unwrap();
        assert_eq!(out["aime"], 8_333.0);
        assert_eq!(out["pia"], 3_313.1);
        assert_eq!(out["monthly_benefit"], 3_313.0);
        assert_eq!(out["full_retirement_age"]["years"], 67);
        // At 62: 3,313.10 * 0.70 = 2,319.17 -> 2,319. At 70: * 1.24 = 4,108.24 -> 4,108.
        assert_eq!(out["monthly_benefit_by_claim_age"]["62"], 2_319.0);
        assert_eq!(out["monthly_benefit_by_claim_age"]["70"], 4_108.0);
        let early =
            run(&json!({"birth_year": 1965, "claim_age": 62, "current_salary": 100_000})).unwrap();
        assert_eq!(early["monthly_benefit"], 2_319.0);
        assert_eq!(early["annual_benefit"], 27_828.0);
    }

    #[test]
    fn a_salary_above_the_wage_base_is_capped_and_a_short_career_counts_less() {
        let high = run(&json!({"birth_year": 1970, "current_salary": 400_000})).unwrap();
        // $184,500 / 12 = 15,375.
        assert_eq!(high["aime"], 15_375.0);
        assert!(high["notes"].to_string().contains("above the wage base"));
        // 20 of 35 years at $60,000: floor(60,000*20/420) = 2,857.
        let short = run(&json!({"birth_year": 1990, "current_salary": 60_000, "career_years": 20}))
            .unwrap();
        assert_eq!(short["aime"], 2_857.0);
    }

    #[test]
    fn an_earnings_history_is_wage_indexed() {
        // Born 1962: indexing year 2022, eligible 2024. Earning exactly the
        // average wage each year 1988 to 2022 indexes every year to AWI(2022)
        // = 63,795.13, so AIME = floor(63,795.13 / 12) = 5,316.
        let history: Vec<Value> = (1988..=2022)
            .map(|y| json!({"year": y, "amount": facts::awi(y).0}))
            .collect();
        let out = run(&json!({"birth_year": 1962, "claim_age": 67, "earnings": history})).unwrap();
        assert_eq!(out["aime"], 5_316.0);
        // 2024 bend points 1,174 / 7,078: 0.9*1,174 + 0.32*(5,316-1,174) = 2,382.04 -> 2,382.00.
        assert_eq!(out["pia"], 2_382.0);
        assert_eq!(out["bend_points"]["first"], 1_174.0);
        // Born 1960 or later: full retirement age 67.
        assert_eq!(out["full_retirement_age"]["years"], 67);
        assert_eq!(out["monthly_benefit"], 2_382.0);
    }

    #[test]
    fn a_short_history_counts_missing_years_as_zero_and_bad_input_is_refused() {
        let out =
            run(&json!({"birth_year": 1962, "earnings": [{"year": 2022, "amount": 63_795.13}]}))
                .unwrap();
        // One year of 63,795.13 over 420 months = 151.
        assert_eq!(out["aime"], 151.0);
        assert!(out["notes"].to_string().contains("count as zero"));
        assert!(run(&json!({"birth_year": 1962})).is_err());
        assert!(run(&json!({"birth_year": 1962, "current_salary": 1, "claim_age": 61})).is_err());
        assert!(
            run(&json!({"birth_year": 1962, "earnings": [{"year": 1970, "amount": 1}]})).is_err()
        );
        assert!(run(&json!({"birth_year": 1962, "earnings": [{"year": 2000, "amount": 1}, {"year": 2000, "amount": 2}]})).is_err());
    }
}
