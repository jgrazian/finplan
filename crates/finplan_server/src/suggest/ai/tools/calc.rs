//! `finance_calc(op)`: loan payments, compounding, pay periods and ages, as
//! exact arithmetic the model does not have to do in its head.

use jiff::civil::Date;
use jiff::{Span, Unit};
use serde_json::{Value, json};

pub const OPS: &[&str] = &[
    "pmt",
    "future_value",
    "present_value",
    "period_to_annual",
    "annual_to_period",
    "age_to_date",
    "date_to_age",
];

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "op": {"type": "string", "enum": OPS, "description": "pmt: level payment on a loan (principal, annual_rate, years). future_value: growth of a lump sum and/or regular payments (annual_rate, years, present_value?, payment?). present_value: what a future amount and/or payment stream is worth now (annual_rate, years, future_value?, payment?). period_to_annual and annual_to_period: pay periods (amount or annual, period). age_to_date: the date someone born birth_date reaches age (years, months?). date_to_age: age on date."},
            "principal": {"type": "number", "description": "pmt: the amount borrowed."},
            "annual_rate": {"type": "number", "description": "A fraction (0.065 for 6.5%), nominal annual, compounded periods_per_year times."},
            "years": {"type": "number", "description": "Term or horizon in years (may be fractional)."},
            "periods_per_year": {"type": "integer", "description": "Payments or compounding periods a year, default 12."},
            "present_value": {"type": "number"},
            "future_value": {"type": "number"},
            "payment": {"type": "number", "description": "A regular payment each period."},
            "timing": {"type": "string", "enum": ["end", "begin"], "description": "When in each period a payment falls; default end."},
            "amount": {"type": "number", "description": "period_to_annual: the pay per period."},
            "annual": {"type": "number", "description": "annual_to_period: the yearly amount."},
            "period": {"type": "string", "enum": ["weekly", "biweekly", "semimonthly", "monthly"]},
            "birth_date": {"type": "string", "description": "YYYY-MM-DD."},
            "age": {"type": "number", "description": "age_to_date: whole years, or with a fraction."},
            "date": {"type": "string", "description": "date_to_age: YYYY-MM-DD."}
        },
        "required": ["op"]
    })
}

fn num(input: &Value, key: &str) -> Result<f64, String> {
    input
        .get(key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("`{key}` is required and must be a number"))
}

fn opt(input: &Value, key: &str) -> Result<Option<f64>, String> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|v| v.is_finite())
            .map(Some)
            .ok_or_else(|| format!("`{key}` must be a number")),
    }
}

fn periods(input: &Value) -> Result<f64, String> {
    match input.get("periods_per_year") {
        None | Some(Value::Null) => Ok(12.0),
        Some(v) => v
            .as_i64()
            .filter(|n| (1..=366).contains(n))
            .map(|n| n as f64)
            .ok_or_else(|| "periods_per_year must be an integer from 1 to 366".to_owned()),
    }
}

fn rate(input: &Value) -> Result<f64, String> {
    let r = num(input, "annual_rate")?;
    if !(-0.5..=2.0).contains(&r) {
        return Err("annual_rate is a fraction such as 0.065, between -0.5 and 2".into());
    }
    Ok(r)
}

fn years(input: &Value) -> Result<f64, String> {
    let y = num(input, "years")?;
    if !(0.0..=100.0).contains(&y) {
        return Err("years must be between 0 and 100".into());
    }
    Ok(y)
}

fn date(input: &Value, key: &str) -> Result<Date, String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("`{key}` is required (YYYY-MM-DD)"))?
        .parse::<Date>()
        .map_err(|_| format!("`{key}` must be a date written YYYY-MM-DD"))
}

/// The level payment that repays `principal` over `n` periods at per-period
/// rate `i`, paid at the end of each period.
pub fn pmt(principal: f64, i: f64, n: f64) -> f64 {
    if n <= 0.0 {
        return principal;
    }
    if i.abs() < 1e-12 {
        return principal / n;
    }
    principal * i / (1.0 - (1.0 + i).powf(-n))
}

/// Growth of `pv` plus a stream of `payment`s over `n` periods at rate `i`.
pub fn future_value(pv: f64, payment: f64, i: f64, n: f64, begin: bool) -> f64 {
    let growth = (1.0 + i).powf(n);
    let annuity = if i.abs() < 1e-12 {
        payment * n
    } else {
        payment * (growth - 1.0) / i
    };
    pv * growth + annuity * if begin { 1.0 + i } else { 1.0 }
}

/// Worth today of `fv` due in `n` periods plus a stream of `payment`s.
pub fn present_value(fv: f64, payment: f64, i: f64, n: f64, begin: bool) -> f64 {
    let discount = (1.0 + i).powf(-n);
    let annuity = if i.abs() < 1e-12 {
        payment * n
    } else {
        payment * (1.0 - discount) / i
    };
    fv * discount + annuity * if begin { 1.0 + i } else { 1.0 }
}

/// Pay periods a year.
pub fn periods_in_year(period: &str) -> Option<f64> {
    match period {
        "weekly" => Some(52.0),
        "biweekly" => Some(26.0),
        "semimonthly" => Some(24.0),
        "monthly" => Some(12.0),
        _ => None,
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

pub fn run(input: &Value) -> Result<Value, String> {
    let op = input
        .get("op")
        .and_then(Value::as_str)
        .ok_or("op is required")?;
    let begin = input.get("timing").and_then(Value::as_str) == Some("begin");
    match op {
        "pmt" => {
            let principal = num(input, "principal")?;
            let (r, y, m) = (rate(input)?, years(input)?, periods(input)?);
            let n = (y * m).round();
            if n < 1.0 {
                return Err("the term must be at least one period".into());
            }
            let payment = pmt(principal, r / m, n);
            Ok(json!({
                "op": op, "payment_per_period": round2(payment), "periods": n,
                "payments_per_year": m, "annual_payments": round2(payment * m),
                "total_paid": round2(payment * n), "total_interest": round2(payment * n - principal),
                "method": "level payment at the end of each period: P*i / (1 - (1+i)^-n), i = annual_rate / periods_per_year",
            }))
        }
        "future_value" => {
            let (r, y, m) = (rate(input)?, years(input)?, periods(input)?);
            let pv = opt(input, "present_value")?.unwrap_or(0.0);
            let pay = opt(input, "payment")?.unwrap_or(0.0);
            let n = y * m;
            let fv = future_value(pv, pay, r / m, n, begin);
            Ok(json!({
                "op": op, "future_value": round2(fv), "periods": n,
                "contributed": round2(pv + pay * n.round()),
                "method": "present_value*(1+i)^n plus the payment annuity, i = annual_rate / periods_per_year",
            }))
        }
        "present_value" => {
            let (r, y, m) = (rate(input)?, years(input)?, periods(input)?);
            let fv = opt(input, "future_value")?.unwrap_or(0.0);
            let pay = opt(input, "payment")?.unwrap_or(0.0);
            let n = y * m;
            let pv = present_value(fv, pay, r / m, n, begin);
            Ok(json!({
                "op": op, "present_value": round2(pv), "periods": n,
                "method": "future_value/(1+i)^n plus the discounted payment annuity, i = annual_rate / periods_per_year",
            }))
        }
        "period_to_annual" | "annual_to_period" => {
            let period = input
                .get("period")
                .and_then(Value::as_str)
                .ok_or("period is required: weekly, biweekly, semimonthly or monthly")?;
            let per = periods_in_year(period)
                .ok_or("period must be weekly, biweekly, semimonthly or monthly")?;
            if op == "period_to_annual" {
                let amount = num(input, "amount")?;
                Ok(json!({
                    "op": op, "period": period, "periods_per_year": per,
                    "annual": round2(amount * per), "monthly": round2(amount * per / 12.0),
                    "method": "amount x periods per year (weekly 52, biweekly 26, semimonthly 24, monthly 12)",
                }))
            } else {
                let annual = num(input, "annual")?;
                Ok(json!({
                    "op": op, "period": period, "periods_per_year": per,
                    "per_period": round2(annual / per),
                    "method": "annual / periods per year",
                }))
            }
        }
        "age_to_date" => {
            let birth = date(input, "birth_date")?;
            let age = num(input, "age")?;
            if !(0.0..=130.0).contains(&age) {
                return Err("age must be between 0 and 130".into());
            }
            let whole = age.floor();
            let months = ((age - whole) * 12.0).round();
            let span = Span::new().years(whole as i64).months(months as i64);
            let at = birth
                .checked_add(span)
                .map_err(|e| format!("date out of range: {e}"))?;
            Ok(json!({
                "op": op, "birth_date": birth.to_string(), "age": age, "date": at.to_string(),
                "year": at.year(),
            }))
        }
        "date_to_age" => {
            let birth = date(input, "birth_date")?;
            let on = date(input, "date")?;
            if on < birth {
                return Err("date is before birth_date".into());
            }
            let span = birth
                .until((Unit::Year, on))
                .map_err(|e| format!("date out of range: {e}"))?;
            let (y, m, d) = (span.get_years(), span.get_months(), span.get_days());
            Ok(json!({
                "op": op, "birth_date": birth.to_string(), "date": on.to_string(),
                "years": y, "months": m, "days": d,
                "age_decimal": ((f64::from(y) + f64::from(m) / 12.0 + f64::from(d) / 365.25) * 100.0).round() / 100.0,
                "year_of_birth": birth.year(),
            }))
        }
        other => Err(format!(
            "unknown op `{other}`; use one of {}",
            OPS.join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thirty_year_mortgage_payment() {
        // $400,000 at 6% for 30 years: $2,398.20 a month (standard amortisation table).
        let out =
            run(&json!({"op": "pmt", "principal": 400_000, "annual_rate": 0.06, "years": 30}))
                .unwrap();
        assert_eq!(out["payment_per_period"], 2_398.20);
        assert_eq!(out["periods"], 360.0);
        let paid = out["total_paid"].as_f64().unwrap();
        assert!((paid - 863_353.0).abs() < 5.0, "{paid}");
        // No interest: the loan divided evenly.
        let zero =
            run(&json!({"op": "pmt", "principal": 12_000, "annual_rate": 0, "years": 1})).unwrap();
        assert_eq!(zero["payment_per_period"], 1_000.0);
    }

    #[test]
    fn compounding_both_ways() {
        // $10,000 at 7% compounded yearly for 10 years: 10,000 * 1.07^10 = 19,671.51.
        let fv = run(&json!({"op": "future_value", "present_value": 10_000, "annual_rate": 0.07, "years": 10, "periods_per_year": 1})).unwrap();
        assert_eq!(fv["future_value"], 19_671.51);
        // $1,000 a year at 5% for 3 years, paid at year end: 1,000 * (1.05^3 - 1)/0.05 = 3,152.50.
        let ann = run(&json!({"op": "future_value", "payment": 1_000, "annual_rate": 0.05, "years": 3, "periods_per_year": 1})).unwrap();
        assert_eq!(ann["future_value"], 3_152.5);
        // Paid at the beginning of each year it is 1.05 times as much.
        let due = run(&json!({"op": "future_value", "payment": 1_000, "annual_rate": 0.05, "years": 3, "periods_per_year": 1, "timing": "begin"})).unwrap();
        assert!((due["future_value"].as_f64().unwrap() - 3_310.125).abs() < 0.006);
        // Discounting undoes it.
        let pv = run(&json!({"op": "present_value", "future_value": 19_671.51, "annual_rate": 0.07, "years": 10, "periods_per_year": 1})).unwrap();
        assert!((pv["present_value"].as_f64().unwrap() - 10_000.0).abs() < 0.01);
    }

    #[test]
    fn pay_periods() {
        // The pay stub in the spec: $5,461.54 biweekly is $142,000 a year.
        let out = run(&json!({"op": "period_to_annual", "amount": 5_461.54, "period": "biweekly"}))
            .unwrap();
        assert_eq!(out["annual"], 142_000.04);
        let back =
            run(&json!({"op": "annual_to_period", "annual": 142_000, "period": "semimonthly"}))
                .unwrap();
        assert_eq!(back["per_period"], 5_916.67);
        assert!(run(&json!({"op": "period_to_annual", "amount": 1, "period": "daily"})).is_err());
    }

    #[test]
    fn ages_and_dates() {
        let d =
            run(&json!({"op": "age_to_date", "birth_date": "1980-05-15", "age": 59.5})).unwrap();
        assert_eq!(d["date"], "2039-11-15");
        let a =
            run(&json!({"op": "date_to_age", "birth_date": "1980-05-15", "date": "2026-09-28"}))
                .unwrap();
        assert_eq!(
            (
                a["years"].as_i64(),
                a["months"].as_i64(),
                a["days"].as_i64()
            ),
            (Some(46), Some(4), Some(13))
        );
        assert!(
            run(&json!({"op": "date_to_age", "birth_date": "2000-01-01", "date": "1999-01-01"}))
                .is_err()
        );
        assert!(run(&json!({"op": "age_to_date", "birth_date": "soon", "age": 60})).is_err());
        assert!(run(&json!({"op": "wat"})).is_err());
    }
}
