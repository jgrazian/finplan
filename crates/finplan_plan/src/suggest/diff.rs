//! Server-rendered diff lines: `Plan › Home Purchase › effects › Sweep`,
//! `Portfolio › GOOG › return profile`.
//!
//! The diff is structural over the before/after GET bodies, so it shows what
//! the batch actually does rather than what its author said it does. Labels
//! and value formats are table-driven by field name: extend [`key_label`] and
//! [`format_of`] for new fields, and the `render_*` functions for new kinds.

use std::collections::HashMap;

use serde_json::Value;

use super::pointer::approx_eq;
use super::{ChangeTarget, Delta, DiffLine};
use crate::graph::ScenarioGraph;

/// Display names for the ids a body can hold.
#[derive(Debug, Clone, Default)]
pub struct Names {
    accounts: HashMap<i64, String>,
    assets: HashMap<i64, String>,
    events: HashMap<i64, String>,
    profiles: HashMap<i64, String>,
    parameters: HashMap<i64, String>,
    tax_configs: HashMap<i64, String>,
    inflation_profiles: HashMap<i64, String>,
}

impl Names {
    pub fn from_graph(graph: &ScenarioGraph) -> Self {
        Names {
            accounts: graph
                .accounts
                .iter()
                .map(|a| (a.id, a.name.clone()))
                .collect(),
            assets: graph
                .assets
                .iter()
                .map(|a| (a.id, a.name.clone()))
                .collect(),
            events: graph
                .events
                .iter()
                .map(|e| (e.id, e.name.clone()))
                .collect(),
            profiles: graph
                .return_profiles
                .values()
                .map(|p| (p.id, p.name.clone()))
                .collect(),
            parameters: graph
                .parameters
                .iter()
                .map(|p| (p.id, p.name.clone()))
                .collect(),
            // The scenario's own, then the rest of the library where the
            // graph carries it.
            tax_configs: graph
                .tax_config
                .iter()
                .map(|c| (c.id, c.name.clone()))
                .chain(
                    graph
                        .tax_configs
                        .values()
                        .map(|c| (c.config.id, c.config.name.clone())),
                )
                .collect(),
            inflation_profiles: graph
                .scenario
                .inflation_profile_id
                .zip(graph.inflation_profile_name.clone())
                .into_iter()
                .chain(
                    graph
                        .inflation_profiles
                        .iter()
                        .map(|(id, p)| (*id, p.name.clone())),
                )
                .collect(),
        }
    }

    /// Add tax config and inflation profile names the graph lacks — a run
    /// snapshot keeps only the scenario's own.
    pub fn with_assumptions(
        mut self,
        tax_configs: impl IntoIterator<Item = (i64, String)>,
        inflation_profiles: impl IntoIterator<Item = (i64, String)>,
    ) -> Self {
        self.tax_configs.extend(tax_configs);
        self.inflation_profiles.extend(inflation_profiles);
        self
    }

    /// Add return profile names the graph lacks — a run snapshot keeps only
    /// the profiles something used.
    pub fn with_profiles(mut self, profiles: impl IntoIterator<Item = (i64, String)>) -> Self {
        self.profiles.extend(profiles);
        self
    }

    fn name(map: &HashMap<i64, String>, id: &Value, what: &str) -> String {
        match id {
            // A resource the batch creates, already rendered by its name.
            Value::String(name) => name.clone(),
            _ => match id.as_i64() {
                Some(id) => map
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| format!("{what} #{id}")),
                None => "none".into(),
            },
        }
    }
}

pub(super) fn lines(delta: &Delta, names: &Names) -> Vec<DiffLine> {
    let name_of = |v: &Option<Value>| {
        v.as_ref()
            .and_then(|v| v.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let (section, fallback) = match &delta.target {
        ChangeTarget::Parameter(id) => ("Parameters", names.parameters.get(id).cloned()),
        ChangeTarget::NewParameter(key) => ("Parameters", Some(key.clone())),
        ChangeTarget::Scenario => ("Scenario", None),
        ChangeTarget::NewReturnProfile(key) | ChangeTarget::NewTaxConfig(key) => {
            ("Assumptions", Some(key.clone()))
        }
        ChangeTarget::Event(id) => ("Plan", names.events.get(id).cloned()),
        ChangeTarget::NewEvent(key) => ("Plan", Some(key.clone())),
        ChangeTarget::Asset(id) => ("Portfolio", names.assets.get(id).cloned()),
        ChangeTarget::Account(id) => ("Portfolio", names.accounts.get(id).cloned()),
        ChangeTarget::NewAsset(key) | ChangeTarget::NewAccount(key) => {
            ("Portfolio", Some(key.clone()))
        }
    };
    let name = name_of(&delta.before)
        .or_else(|| name_of(&delta.after))
        .or(fallback)
        .unwrap_or_default();
    // Parameters read as they are written in an expression.
    let name = match &delta.target {
        ChangeTarget::Parameter(_) | ChangeTarget::NewParameter(_) => format!("${name}"),
        _ => name,
    };
    // A resource the batch brings into being reads as an addition; the
    // scenario's settings are the plan itself, so they need no name.
    let root = if delta.target == ChangeTarget::Scenario {
        section.to_string()
    } else if delta.before.is_none() && delta.after.is_some() {
        format!("{section} › + {name}")
    } else {
        format!("{section} › {name}")
    };

    // A parameter's value is a tagged object; show it as one figure.
    let flatten = |v: &Option<Value>| match (&delta.target, v) {
        (ChangeTarget::Parameter(_) | ChangeTarget::NewParameter(_), Some(v)) => {
            Some(flatten_parameter(v))
        }
        _ => v.clone(),
    };
    let (before, after) = (flatten(&delta.before), flatten(&delta.after));

    let mut out = Vec::new();
    match (&before, &after) {
        (Some(before), Some(after)) => walk(before, after, &root, "", names, &mut out),
        (before, after) => out.push(DiffLine {
            label: root,
            from: before.as_ref().map(|v| summary(&delta.target, v, names)),
            to: after.as_ref().map(|v| summary(&delta.target, v, names)),
        }),
    }
    out
}

/// A parameter body with its tagged `value` rendered as the one figure it is:
/// `$10,000`, `4%`, `2031-06-01`, `67 years`.
fn flatten_parameter(body: &Value) -> Value {
    let mut body = body.clone();
    if let Some(object) = body.as_object_mut()
        && let Some(value) = object.get("value")
    {
        let text = match value.get("kind").and_then(Value::as_str) {
            Some("Money") => money(field(value, "value").as_f64().unwrap_or_default()),
            Some("Rate") => percent(field(value, "value").as_f64().unwrap_or_default()),
            Some("Date") => field(value, "value")
                .as_str()
                .unwrap_or_default()
                .to_string(),
            Some("Age") => {
                let years = field(value, "years").as_i64().unwrap_or_default();
                match field(value, "months").as_i64().unwrap_or_default() {
                    0 => format!("{years} years"),
                    months => format!("{years} years {months} months"),
                }
            }
            _ => value.to_string(),
        };
        object.insert("value".into(), Value::String(text));
    }
    body
}

/// A new return profile's distribution, in a line: `Bootstrap sp500`,
/// `Normal 7% ± 15%`.
fn distribution(value: &Value) -> String {
    let number = |key: &str| field(value, key).as_f64().unwrap_or_default();
    match value
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "Fixed" => format!("fixed {}", percent(number("rate"))),
        kind @ ("Normal" | "LogNormal") => format!(
            "{} {} ± {}",
            kind.to_lowercase(),
            percent(number("mean")),
            percent(number("std_dev"))
        ),
        "StudentT" => format!(
            "Student t {} (scale {})",
            percent(number("mean")),
            percent(number("scale"))
        ),
        "RegimeSwitching" => "bull/bear regimes".to_string(),
        "Bootstrap" => format!(
            "history: {}",
            field(value, "preset").as_str().unwrap_or_default()
        ),
        _ => "no return".to_string(),
    }
}

/// A new tax config, in a line:
/// `state 5% · capital gains 15% · 7 brackets · $14,600 deduction`.
fn new_tax_config(value: &Value) -> String {
    let rate = |key: &str| percent(field(value, key).as_f64().unwrap_or_default());
    let brackets = field(value, "federal_brackets")
        .as_array()
        .map_or(0, Vec::len);
    let mut line = format!(
        "state {} · capital gains {} · {brackets} brackets",
        rate("state_rate"),
        rate("capital_gains_rate")
    );
    let deduction = field(value, "standard_deduction")
        .as_f64()
        .unwrap_or_default();
    if deduction > 0.0 {
        line.push_str(&format!(" · {} deduction", money(deduction)));
    }
    line
}

/// A whole resource, for a line that creates or deletes one.
fn summary(target: &ChangeTarget, value: &Value, names: &Names) -> String {
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match target {
        // A creation's label already names it: describe what it is.
        ChangeTarget::NewAsset(_) => new_asset(value, names),
        ChangeTarget::NewAccount(_) => new_account(value, names),
        ChangeTarget::NewReturnProfile(_) => distribution(field(value, "distribution")),
        ChangeTarget::NewTaxConfig(_) => new_tax_config(value),
        // The body already carries its figure, rendered by `flatten_parameter`.
        ChangeTarget::Parameter(_) | ChangeTarget::NewParameter(_) => field(value, "value")
            .as_str()
            .unwrap_or_default()
            .to_string(),
        ChangeTarget::Scenario => name.to_string(),
        ChangeTarget::Event(_) | ChangeTarget::NewEvent(_) => {
            let effects = value
                .get("effects")
                .and_then(Value::as_array)
                .map(|e| {
                    e.iter()
                        .map(|e| render_effect(e, names))
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .unwrap_or_default();
            let trigger = value
                .get("trigger")
                .map(|t| render_trigger(t, names))
                .unwrap_or_default();
            if effects.is_empty() {
                format!("{name} · {trigger}")
            } else {
                format!("{name} · {trigger} · {effects}")
            }
        }
        ChangeTarget::Asset(_) => format!(
            "{name} · {}",
            render_scalar("return_profile_id", &value["return_profile_id"], names)
        ),
        ChangeTarget::Account(_) => format!(
            "{name} ({})",
            value
                .get("flavor")
                .and_then(Value::as_str)
                .unwrap_or("account")
        ),
    }
}

/// A new asset: its price and what moves it, e.g. `$250.00 · US Total Market`.
fn new_asset(value: &Value, names: &Names) -> String {
    let price = money_cents(field(value, "initial_price").as_f64().unwrap_or(1.0));
    let profile = match field(value, "return_profile_id") {
        Value::Null => "no return assumption".to_string(),
        id => Names::name(&names.profiles, id, "profile"),
    };
    let mut parts = vec![price, profile];
    if let Some(te) = field(value, "tracking_error").as_f64() {
        parts.push(format!("{} tracking error", percent(te)));
    }
    parts.join(" · ")
}

/// A new account by flavor, e.g. `Taxable investment · $50,000 cash · 2 lots`.
fn new_account(value: &Value, names: &Names) -> String {
    let money_of = |key: &str| money(field(value, key).as_f64().unwrap_or_default());
    match field(value, "flavor").as_str().unwrap_or_default() {
        "Investment" => {
            let status = match field(value, "tax_status").as_str() {
                Some("TaxDeferred") => "Tax-deferred",
                Some("TaxFree") => "Tax-free",
                _ => "Taxable",
            };
            let kind = match field(value, "plan_type").as_str() {
                Some("Traditional401k") => "Traditional 401(k)".to_string(),
                Some("Roth401k") => "Roth 401(k)".to_string(),
                Some("TraditionalIra") => "Traditional IRA".to_string(),
                Some("RothIra") => "Roth IRA".to_string(),
                Some("Hsa") => "HSA".to_string(),
                _ => format!("{status} investment"),
            };
            let lots = field(value, "positions").as_array().map_or(0, Vec::len);
            let mut parts = vec![kind, format!("{} cash", money_of("cash_value"))];
            if lots > 0 {
                parts.push(if lots == 1 {
                    "1 lot".to_string()
                } else {
                    format!("{lots} lots")
                });
            }
            if let Some(limit) = field(value, "contribution_limit").as_f64() {
                let period = match field(value, "contribution_period").as_str() {
                    Some("Monthly") => "month",
                    _ => "year",
                };
                let catch_up: Vec<String> = field(value, "catch_up")
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|tier| {
                        let amount = field(tier, "amount").as_f64()?;
                        let from = field(tier, "from_age").as_u64()?;
                        Some(match field(tier, "through_age").as_u64() {
                            Some(through) => format!("+{} at {from}-{through}", money(amount)),
                            None => format!("+{} at {from}+", money(amount)),
                        })
                    })
                    .collect();
                let extra = if catch_up.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", catch_up.join(", "))
                };
                parts.push(format!("up to {} a {period}{extra}", money(limit)));
            }
            parts.join(" · ")
        }
        "Bank" => format!(
            "Bank · {} cash · {}",
            money_of("cash_value"),
            Names::name(
                &names.profiles,
                field(value, "return_profile_id"),
                "profile"
            )
        ),
        "Property" => format!(
            "Property · {} · {}",
            Names::name(&names.assets, field(value, "asset_id"), "asset"),
            money_of("value")
        ),
        "Liability" => {
            let mut text = format!(
                "Liability · {} at {}",
                money_of("principal"),
                percent(field(value, "interest_rate").as_f64().unwrap_or_default())
            );
            if let Some(repayment) = value.get("repayment").filter(|r| r.is_object()) {
                text.push_str(&format!(
                    " · repaid from {} over {}",
                    Names::name(
                        &names.accounts,
                        field(repayment, "from_account_id"),
                        "account"
                    ),
                    render_scalar("term_months", field(repayment, "term_months"), names)
                ));
            }
            text
        }
        other => format!("{other} account"),
    }
}

/// Keys whose value is a transfer amount: shown whole, never field by field,
/// since `$20,000, inflation-adjusted` reads better than `inner › value`.
const AMOUNT_KEYS: &[&str] = &["amount", "price", "down_payment", "left", "right", "inner"];

fn walk(
    before: &Value,
    after: &Value,
    label: &str,
    key: &str,
    names: &Names,
    out: &mut Vec<DiffLine>,
) {
    if approx_eq(before, after) {
        return;
    }
    match (before, after) {
        (Value::Object(b), Value::Object(a))
            if !AMOUNT_KEYS.contains(&key) && same_tag(before, after) =>
        {
            let keys = b.keys().chain(a.keys().filter(|k| !b.contains_key(*k)));
            for k in keys {
                let child = format!("{label} › {}", key_label(k));
                match (b.get(k), a.get(k)) {
                    (Some(bv), Some(av)) => walk(bv, av, &child, k, names, out),
                    (bv, av) => out.push(DiffLine {
                        label: child,
                        from: bv.map(|v| render(k, v, names)),
                        to: av.map(|v| render(k, v, names)),
                    }),
                }
            }
        }
        (Value::Array(b), Value::Array(a)) => walk_list(b, a, label, key, names, out),
        _ => out.push(DiffLine {
            label: label.to_string(),
            from: Some(render(key, before, names)),
            to: Some(render(key, after, names)),
        }),
    }
}

/// Two objects of the same kind: field-by-field is meaningful between them.
fn same_tag(a: &Value, b: &Value) -> bool {
    ["kind", "mode", "flavor"]
        .iter()
        .all(|tag| a.get(tag) == b.get(tag))
}

/// Diff two lists by their longest common subsequence: unmatched items
/// between anchors pair up (and diff field by field) where they are the same
/// kind, and otherwise read as removed and added.
fn walk_list(
    b: &[Value],
    a: &[Value],
    label: &str,
    key: &str,
    names: &Names,
    out: &mut Vec<DiffLine>,
) {
    let (n, m) = (b.len(), a.len());
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if approx_eq(&b[i], &a[j]) {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let item_label = |v: &Value| format!("{label} › {}", element_label(key, v, names));
    let flush = |removed: &mut Vec<&Value>, added: &mut Vec<&Value>, out: &mut Vec<DiffLine>| {
        let mut added_iter = std::mem::take(added).into_iter().peekable();
        for old in std::mem::take(removed) {
            match added_iter.peek() {
                Some(new) if same_tag(old, new) && old.is_object() => {
                    let new = added_iter.next().expect("peeked");
                    walk(old, new, &item_label(old), key, names, out);
                }
                _ => out.push(DiffLine {
                    label: item_label(old),
                    from: Some(render(key, old, names)),
                    to: None,
                }),
            }
        }
        for new in added_iter {
            out.push(DiffLine {
                label: item_label(new),
                from: None,
                to: Some(render(key, new, names)),
            });
        }
    };

    let (mut i, mut j) = (0, 0);
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    while i < n || j < m {
        if i < n && j < m && approx_eq(&b[i], &a[j]) {
            flush(&mut removed, &mut added, out);
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            added.push(&a[j]);
            j += 1;
        } else {
            removed.push(&b[i]);
            i += 1;
        }
    }
    flush(&mut removed, &mut added, out);
}

/// How one item of a list is named in a label.
fn element_label(key: &str, value: &Value, names: &Names) -> String {
    let kind = || {
        value
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("item")
            .to_string()
    };
    match key {
        "effects" | "children" => kind(),
        "positions" => Names::name(&names.assets, &value["asset_id"], "asset"),
        "exclude_accounts" => Names::name(&names.accounts, value, "account"),
        "entries" => Names::name(&names.accounts, &value["account_id"], "account"),
        _ => key_label(key).to_string(),
    }
}

/// The label segment for a field.
fn key_label(key: &str) -> String {
    let label = match key {
        "return_profile_id" => "return profile",
        "cash_return_profile_id" => "cash return profile",
        "initial_price" => "price",
        "tracking_error" => "tracking error",
        "cost_basis" => "cost basis",
        "purchase_date" => "purchase date",
        "fires_once" => "fires once",
        "sort_order" => "position in list",
        "to_account_id" => "to",
        "from_account_id" => "from",
        "account_id" => "account",
        "asset_id" => "asset",
        "event_id" => "event",
        "target_event_id" => "target event",
        "parameter_id" => "parameter",
        "property_account_id" => "property",
        "payoff_account_id" => "pays off",
        "loan_account_id" => "loan",
        "pay_tax_from_account_id" => "pays tax from",
        "start_condition" => "starts",
        "end_condition" => "ends",
        "on_true" => "if true",
        "on_false" => "if false",
        "on_date" => "date",
        "cash_value" => "cash",
        "term_months" => "term",
        "exclude_accounts" => "excluded accounts",
        "tax_config_id" => "tax config",
        "inflation_profile_id" => "inflation profile",
        "duration_years" => "duration (years)",
        "birth_date" => "birth date",
        other => return other.replace('_', " "),
    };
    label.to_string()
}

#[derive(Clone, Copy)]
enum Format {
    Money,
    Percent,
    Months,
    Account,
    Asset,
    Event,
    Profile,
    Parameter,
    TaxConfig,
    Inflation,
    Plain,
}

/// How a scalar field's value is shown.
fn format_of(key: &str) -> Format {
    match key {
        "account_id"
        | "to_account_id"
        | "from_account_id"
        | "property_account_id"
        | "payoff_account_id"
        | "loan_account_id"
        | "pay_tax_from_account_id"
        | "exclude_accounts" => Format::Account,
        "asset_id" => Format::Asset,
        "event_id" | "target_event_id" => Format::Event,
        "return_profile_id" | "cash_return_profile_id" => Format::Profile,
        "parameter_id" => Format::Parameter,
        "tax_config_id" => Format::TaxConfig,
        "inflation_profile_id" => Format::Inflation,
        "threshold" | "cash_value" | "principal" | "cost_basis" | "initial_price"
        | "contribution_limit" | "gain_exclusion" => Format::Money,
        "interest_rate" | "drop" | "probability" | "selling_cost_rate" | "tracking_error"
        | "bracket_ceiling" => Format::Percent,
        "term_months" => Format::Months,
        _ => Format::Plain,
    }
}

/// Any value, read in the context of the field that holds it.
fn render(key: &str, value: &Value, names: &Names) -> String {
    match key {
        k if AMOUNT_KEYS.contains(&k) => render_amount(value, names),
        "trigger" | "start_condition" | "end_condition" | "children" if value.is_object() => {
            render_trigger(value, names)
        }
        "effects" | "on_true" | "on_false" if value.is_object() => render_effect(value, names),
        "positions" if value.is_object() => render_position(value, names),
        "sources" if value.is_object() => render_sources(value, names),
        _ => match value {
            Value::Array(items) if items.is_empty() => "none".into(),
            Value::Array(items) => items
                .iter()
                .map(|v| render(key, v, names))
                .collect::<Vec<_>>()
                .join(", "),
            Value::Object(_) => value.to_string(),
            _ => render_scalar(key, value, names),
        },
    }
}

fn render_scalar(key: &str, value: &Value, names: &Names) -> String {
    match value {
        Value::Null => return "none".into(),
        Value::Bool(b) => return if *b { "yes" } else { "no" }.into(),
        Value::String(s) => return s.clone(),
        _ => {}
    }
    let x = value.as_f64().unwrap_or_default();
    match format_of(key) {
        Format::Money => money(x),
        Format::Percent => percent(x),
        Format::Months if x % 12.0 == 0.0 => format!("{} years", x / 12.0),
        Format::Months => format!("{x} months"),
        Format::Account => Names::name(&names.accounts, value, "account"),
        Format::Asset => Names::name(&names.assets, value, "asset"),
        Format::Event => Names::name(&names.events, value, "event"),
        Format::Profile => Names::name(&names.profiles, value, "profile"),
        Format::Parameter => Names::name(&names.parameters, value, "parameter"),
        Format::TaxConfig => Names::name(&names.tax_configs, value, "tax config"),
        Format::Inflation => Names::name(&names.inflation_profiles, value, "inflation profile"),
        Format::Plain => number(x),
    }
}

fn money(x: f64) -> String {
    let sign = if x < 0.0 { "−" } else { "" };
    let x = x.abs();
    if x < 100.0 && x.fract() != 0.0 {
        return format!("{sign}${x:.2}");
    }
    let digits = format!("{:.0}", x);
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{sign}${grouped}")
}

/// Always two decimals: a unit price.
fn money_cents(x: f64) -> String {
    let whole = money(x.trunc());
    format!(
        "{whole}.{:02}",
        ((x.abs().fract()) * 100.0).round() as i64 % 100
    )
}

fn percent(x: f64) -> String {
    format!("{}%", number(x * 100.0))
}

/// Up to two decimals, trailing zeros trimmed.
fn number(x: f64) -> String {
    let text = format!("{x:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&Value::Null)
}

fn render_amount(value: &Value, names: &Names) -> String {
    let sub = |key: &str| render_amount(field(value, key), names);
    let account = || Names::name(&names.accounts, field(value, "account_id"), "account");
    match value
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "Fixed" => money(field(value, "value").as_f64().unwrap_or_default()),
        "InflationAdjusted" => format!("{}, inflation-adjusted", sub("inner")),
        "Scale" => format!(
            "{}× {}",
            number(field(value, "factor").as_f64().unwrap_or(1.0)),
            sub("inner")
        ),
        "Expression" => format!("= {}", field(value, "source").as_str().unwrap_or_default()),
        "SourceBalance" => "the source's balance".into(),
        "ZeroTargetBalance" => "enough to zero the target".into(),
        "TargetToBalance" => format!(
            "enough to bring the target to {}",
            money(field(value, "value").as_f64().unwrap_or_default())
        ),
        "AssetBalance" => format!(
            "{} in {}",
            Names::name(&names.assets, field(value, "asset_id"), "asset"),
            account()
        ),
        "AccountTotalBalance" => format!("{}'s balance", account()),
        "AccountCashBalance" => format!("{}'s cash", account()),
        "Min" => format!("min({}, {})", sub("left"), sub("right")),
        "Max" => format!("max({}, {})", sub("left"), sub("right")),
        "Sub" => format!("{} − {}", sub("left"), sub("right")),
        "Add" => format!("{} + {}", sub("left"), sub("right")),
        "Mul" => format!("{} × {}", sub("left"), sub("right")),
        _ if value.is_null() => "none".into(),
        _ => value.to_string(),
    }
}

fn render_trigger(value: &Value, names: &Names) -> String {
    let comparison = || match field(value, "comparison").as_str() {
        Some("LessThanOrEqual") => "≤",
        _ => "≥",
    };
    let threshold = || money(field(value, "threshold").as_f64().unwrap_or_default());
    let account = || Names::name(&names.accounts, field(value, "account_id"), "account");
    let children = |joiner: &str| {
        field(value, "children")
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| render_trigger(c, names))
            .collect::<Vec<_>>()
            .join(joiner)
    };
    match value
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "Date" => format!(
            "on {}",
            field(value, "on_date").as_str().unwrap_or_default()
        ),
        "Age" => match field(value, "months").as_i64() {
            Some(m) if m > 0 => format!("at age {}y {m}m", field(value, "years")),
            _ => format!("at age {}", field(value, "years")),
        },
        "DateParameter" | "AgeParameter" => {
            Names::name(&names.parameters, field(value, "parameter_id"), "parameter")
        }
        "RelativeToEvent" => {
            let event = Names::name(&names.events, field(value, "event_id"), "event");
            match field(value, "value").as_i64().unwrap_or_default() {
                0 => format!("with {event}"),
                n => format!(
                    "{n} {} after {event}",
                    field(value, "unit")
                        .as_str()
                        .unwrap_or("Months")
                        .to_lowercase()
                ),
            }
        }
        "AccountBalance" => format!("{} {} {}", account(), comparison(), threshold()),
        "AssetBalance" => format!(
            "{} in {} {} {}",
            Names::name(&names.assets, field(value, "asset_id"), "asset"),
            account(),
            comparison(),
            threshold()
        ),
        "NetWorth" => format!("net worth {} {}", comparison(), threshold()),
        "And" => children(" and "),
        "Or" => children(" or "),
        "Repeating" => {
            let mut text = field(value, "interval")
                .as_str()
                .unwrap_or("Monthly")
                .to_string();
            if let Some(start) = value.get("start_condition").filter(|v| !v.is_null()) {
                text.push_str(&format!(", from {}", render_trigger(start, names)));
            }
            if let Some(end) = value.get("end_condition").filter(|v| !v.is_null()) {
                text.push_str(&format!(", until {}", render_trigger(end, names)));
            }
            if let Some(max) = field(value, "max_occurrences").as_i64() {
                text.push_str(&format!(", at most {max} times"));
            }
            text
        }
        "Manual" => "manually".into(),
        _ => value.to_string(),
    }
}

fn render_effect(value: &Value, names: &Names) -> String {
    let amount = |key: &str| render_amount(field(value, key), names);
    let account = |key: &str| Names::name(&names.accounts, field(value, key), "account");
    let event = || Names::name(&names.events, field(value, "target_event_id"), "event");
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match kind {
        "Income" => format!("Income {} → {}", amount("amount"), account("to_account_id")),
        "Expense" => format!(
            "Expense {} from {}",
            amount("amount"),
            account("from_account_id")
        ),
        "AssetPurchase" => format!(
            "Buy {} with {} from {} into {}",
            Names::name(&names.assets, field(value, "asset_id"), "asset"),
            amount("amount"),
            account("from_account_id"),
            account("to_account_id")
        ),
        "AssetSale" => format!(
            "Sell {} from {}",
            amount("amount"),
            account("from_account_id")
        ),
        "Sweep" => format!("Sweep {} → {}", amount("amount"), account("to_account_id")),
        "AdjustBalance" => format!("Adjust {} by {}", account("account_id"), amount("amount")),
        "CashTransfer" => format!(
            "Transfer {} {} → {}",
            amount("amount"),
            account("from_account_id"),
            account("to_account_id")
        ),
        "TriggerEvent" => format!("Trigger {}", event()),
        "PauseEvent" => format!("Pause {}", event()),
        "ResumeEvent" => format!("Resume {}", event()),
        "TerminateEvent" => format!("Terminate {}", event()),
        "DeleteAccount" => format!("Close {}", account("account_id")),
        "ApplyRmd" => format!("RMD → {}", account("to_account_id")),
        "RsuVesting" => format!(
            "Vest {} {} → {}",
            number(field(value, "units").as_f64().unwrap_or_default()),
            Names::name(&names.assets, field(value, "asset_id"), "asset"),
            account("to_account_id")
        ),
        "Random" => format!(
            "{} chance: {}",
            percent(field(value, "probability").as_f64().unwrap_or_default()),
            render_effect(field(value, "on_true"), names)
        ),
        "BuyProperty" => format!(
            "Buy {} for {} from {}",
            account("property_account_id"),
            amount("price"),
            account("from_account_id")
        ),
        "SellProperty" => format!(
            "Sell {} → {}",
            account("property_account_id"),
            account("to_account_id")
        ),
        "MarketShock" => format!(
            "Market shock −{}",
            percent(field(value, "drop").as_f64().unwrap_or_default())
        ),
        "RothConversion" => format!(
            "Convert {} {} → {}, tax {}",
            amount("amount"),
            account("from_account_id"),
            account("to_account_id"),
            if field(value, "pay_tax_from_account_id").is_null() {
                "withheld".to_string()
            } else {
                format!("from {}", account("pay_tax_from_account_id"))
            }
        ),
        _ => value.to_string(),
    }
}

fn render_position(value: &Value, names: &Names) -> String {
    format!(
        "{} × {} · basis {}",
        number(field(value, "units").as_f64().unwrap_or_default()),
        Names::name(&names.assets, field(value, "asset_id"), "asset"),
        money(field(value, "cost_basis").as_f64().unwrap_or_default())
    )
}

fn render_sources(value: &Value, names: &Names) -> String {
    let account = || Names::name(&names.accounts, field(value, "account_id"), "account");
    match value
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "SingleAccount" => account(),
        "SingleAsset" => format!(
            "{} in {}",
            Names::name(&names.assets, field(value, "asset_id"), "asset"),
            account()
        ),
        "Custom" => "a custom order".into(),
        _ => format!(
            "{} strategy",
            field(value, "strategy").as_str().unwrap_or_default()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(money(20000.0), "$20,000");
        assert_eq!(money(-1_081_234.4), "−$1,081,234");
        assert_eq!(money(12.5), "$12.50");
        assert_eq!(percent(0.0337398), "3.37%");
        assert_eq!(percent(0.06), "6%");
    }
}
