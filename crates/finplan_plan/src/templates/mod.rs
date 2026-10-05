//! Templates: plain facts about a household ("a $142k salary until 65", "a
//! 30-year mortgage at 6.5%") lowered to the [`Change`]s that write them.
//!
//! Guided setup and the drafting agent's `expand_template` tool share these
//! builders, so the trigger and effect trees for a salary, an employer match,
//! spending, retirement, a home purchase and Social Security are written once.
//! Every expansion is a list of `add` changes at `""` on `new_*` targets; what
//! one creates is named by `{key_prefix}{local key}`, and any id field of the
//! request may point at something else in the same batch with `{"$new": key}`
//! ([`RowRef`]) or at an existing row by id. Two expansions in one path use
//! different prefixes so their keys cannot collide; keys a caller passes in
//! (`RowRef::New`) are used exactly as given.
//!
//! The lower-level builders ([`bank_account`], [`investment_account`],
//! [`allocation_asset`], [`position`]) are the accounts and holdings those
//! templates hang off; they carry no facts of their own.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use ts_rs::TS;

use crate::error::PlanError;
use crate::specs::Interval;
use crate::specs::parameters::ParameterValueSpec;
use crate::suggest::{Change, ChangeOp, ChangeTarget, RefKind};

// ── request types ───────────────────────────────────────────────────────────

/// `{"$new": "<key>"}`: a resource the same batch creates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewRef {
    #[serde(rename = "$new")]
    #[ts(rename = "$new")]
    pub key: String,
}

/// An id field: an existing row's id, or a resource the batch creates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export)]
pub enum RowRef {
    Id(i64),
    New(NewRef),
}

impl RowRef {
    pub fn new(key: impl Into<String>) -> Self {
        RowRef::New(NewRef { key: key.into() })
    }

    fn json(&self) -> Value {
        serde_json::to_value(self).expect("a reference serializes")
    }
}

impl From<i64> for RowRef {
    fn from(id: i64) -> Self {
        RowRef::Id(id)
    }
}

/// When something starts, ends or happens: at an age, at a date, or at an age
/// or date a plan parameter holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum When {
    Age {
        years: u8,
        #[serde(default)]
        #[ts(optional)]
        months: Option<u8>,
    },
    AgeParameter {
        parameter_id: RowRef,
    },
    Date {
        on_date: String,
    },
    DateParameter {
        parameter_id: RowRef,
    },
}

impl When {
    pub fn age(years: u8) -> Self {
        When::Age {
            years,
            months: None,
        }
    }

    fn trigger(&self) -> Result<Value, TemplateError> {
        Ok(match self {
            When::Age { years, months } => {
                if *years > 120 || months.is_some_and(|m| m > 11) {
                    return Err(TemplateError::new("age must be 0-120 years, 0-11 months"));
                }
                let mut trigger = json!({"kind": "Age", "years": years});
                if let Some(months) = months {
                    trigger["months"] = json!(months);
                }
                trigger
            }
            When::AgeParameter { parameter_id } => {
                json!({"kind": "AgeParameter", "parameter_id": parameter_id.json()})
            }
            When::Date { on_date } => {
                on_date
                    .parse::<jiff::civil::Date>()
                    .map_err(|_| TemplateError::new("dates are written YYYY-MM-DD"))?;
                json!({"kind": "Date", "on_date": on_date})
            }
            When::DateParameter { parameter_id } => {
                json!({"kind": "DateParameter", "parameter_id": parameter_id.json()})
            }
        })
    }
}

/// A share of a contribution that buys one asset in the receiving account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Allocation {
    pub asset_id: RowRef,
    /// 0-1; the shares of one contribution add up to 1.
    pub fraction: f64,
}

/// Salary paid into a bank account, optionally with a pre-tax employee
/// contribution to a 401(k) taken out of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SalaryParams {
    /// Defaults to "Salary".
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    pub to_account_id: RowRef,
    /// Gross annual pay, in today's dollars.
    pub annual_amount: f64,
    #[serde(default)]
    #[ts(optional)]
    pub start: Option<When>,
    /// Usually the retirement age.
    #[serde(default)]
    #[ts(optional)]
    pub end: Option<When>,
    #[serde(default)]
    #[ts(optional)]
    pub employee_401k: Option<Employee401k>,
    #[serde(default)]
    #[ts(optional)]
    pub sort_order: Option<i64>,
}

/// The part of a salary the employee defers into a 401(k).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Employee401k {
    pub account_id: RowRef,
    /// Annual deferral, in today's dollars; at most the salary.
    pub annual_amount: f64,
    /// What each deferral buys; empty leaves it as cash in the account.
    #[serde(default)]
    pub allocation: Vec<Allocation>,
}

/// An employer's 401(k) match: tax-free income into the account, a share of
/// salary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EmployerMatchParams {
    /// Defaults to "Employer 401(k) match".
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    pub to_account_id: RowRef,
    /// Gross annual salary, in today's dollars.
    pub salary: f64,
    /// A plan parameter (by name, without the `$`) holding the salary; the
    /// match then follows it instead of `salary`. The parameter must exist or
    /// be created in the same batch.
    #[serde(default)]
    #[ts(optional)]
    pub salary_parameter: Option<String>,
    /// Dollars of match per dollar contributed: 0.5 for "50% of the first 6%".
    pub match_rate: f64,
    /// The contribution the employer matches up to, in percent of salary.
    pub up_to_percent: f64,
    /// What the employee contributes, in percent of salary; defaults to
    /// `up_to_percent`.
    #[serde(default)]
    #[ts(optional)]
    pub employee_percent: Option<f64>,
    #[serde(default)]
    pub allocation: Vec<Allocation>,
    #[serde(default)]
    #[ts(optional)]
    pub start: Option<When>,
    #[serde(default)]
    #[ts(optional)]
    pub end: Option<When>,
    #[serde(default)]
    #[ts(optional)]
    pub sort_order: Option<i64>,
}

/// Spending that repeats: rent, groceries, insurance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RecurringExpenseParams {
    pub name: String,
    pub from_account_id: RowRef,
    /// Per `interval`, in today's dollars unless `inflation_adjusted` is off.
    pub amount: f64,
    /// A Money plan parameter (by name, without the `$`) holding the amount;
    /// the expense then follows it instead of `amount`, which stays what the
    /// expense comes to at the plan's start. The parameter must exist or be
    /// created in the same batch.
    #[serde(default)]
    #[ts(optional)]
    pub amount_parameter: Option<String>,
    /// How often the parameter's amount is spent, when that differs from
    /// `interval`: a monthly figure paid yearly is paid as 12 of it. Defaults
    /// to `interval`.
    #[serde(default)]
    #[ts(optional)]
    pub parameter_interval: Option<Interval>,
    /// Defaults to yearly.
    #[serde(default)]
    #[ts(optional)]
    pub interval: Option<Interval>,
    /// Defaults to on.
    #[serde(default)]
    #[ts(optional)]
    pub inflation_adjusted: Option<bool>,
    #[serde(default)]
    #[ts(optional)]
    pub start: Option<When>,
    #[serde(default)]
    #[ts(optional)]
    pub end: Option<When>,
    /// Refill the paying account from investments before each payment, by the
    /// tax-efficient-early strategy.
    #[serde(default)]
    pub fund_from_investments: bool,
    #[serde(default)]
    #[ts(optional)]
    pub sort_order: Option<i64>,
}

/// A retirement marker event, and optionally the spending that begins with it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RetirementParams {
    pub retirement: When,
    /// Defaults to "Retirement".
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub spending: Option<RetirementSpending>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RetirementSpending {
    /// Defaults to "Retirement spending".
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    pub from_account_id: RowRef,
    /// Per year, in today's dollars.
    pub annual_amount: f64,
    #[serde(default)]
    pub fund_from_investments: bool,
}

/// Buying a home: the property, and a mortgage when the down payment does not
/// cover the price.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HomePurchaseParams {
    /// Names the property account; defaults to "Home".
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    pub price: f64,
    pub down_payment: f64,
    /// Annual rate as a fraction (0.065). Required when financed.
    #[serde(default)]
    #[ts(optional)]
    pub mortgage_rate: Option<f64>,
    /// Defaults to 360.
    #[serde(default)]
    #[ts(optional)]
    pub term_months: Option<u32>,
    /// Pays the down payment and the monthly mortgage payments.
    pub from_account_id: RowRef,
    pub when: When,
    /// Price and down payment are in today's dollars (the default), inflated
    /// to the purchase date.
    #[serde(default)]
    #[ts(optional)]
    pub inflation_adjusted: Option<bool>,
    /// What moves the home's value; unmapped, it stays flat.
    #[serde(default)]
    #[ts(optional)]
    pub appreciation_profile_id: Option<RowRef>,
}

/// Social Security retirement benefits from a claiming age.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SocialSecurityParams {
    /// Defaults to "Social Security".
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    pub to_account_id: RowRef,
    /// Annual benefit at the claiming age, in today's dollars.
    pub annual_benefit: f64,
    pub claim: When,
}

/// A one-time market crash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MarketCrashParams {
    /// Defaults to "Market crash".
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    /// Fraction every market asset's price falls by, between 0 and 1.
    pub drop: f64,
    pub when: When,
    /// Defaults to on.
    #[serde(default)]
    #[ts(optional)]
    pub enabled: Option<bool>,
}

/// A one-time large cost: medical, long-term care, a roof.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LargeExpenseParams {
    /// Defaults to "Large expense".
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    pub from_account_id: RowRef,
    pub amount: f64,
    pub when: When,
    #[serde(default)]
    #[ts(optional)]
    pub inflation_adjusted: Option<bool>,
    #[serde(default)]
    #[ts(optional)]
    pub enabled: Option<bool>,
}

/// Income stops for a while: an event pauses, and resumes `months` later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct JobLossParams {
    /// Names the pair of events; defaults to "Job loss".
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    /// The salary event that pauses.
    pub salary_event_id: RowRef,
    pub when: When,
    pub months: u32,
    #[serde(default)]
    #[ts(optional)]
    pub enabled: Option<bool>,
}

/// Yearly Roth conversions: every Dec 30 from `start` until `end`, convert
/// pre-tax money up to the top of the `ceiling_rate` bracket
/// (`bracket_room(ceiling_rate)`). Year-end, so the year's other income has
/// landed; the event is added last, so it runs after the day's other events.
/// Dec 30 rather than 31: see [`CONVERSION_DAY`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RothConversionsParams {
    /// Defaults to "Roth conversions".
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    /// The tax-deferred account converted from (a 401(k) or traditional IRA).
    pub from_account_id: RowRef,
    /// The tax-free (Roth) account converted into.
    pub to_account_id: RowRef,
    /// The highest marginal rate to fill to, a fraction: 0.12, 0.22, 0.24.
    pub ceiling_rate: f64,
    /// When conversions begin: the first is on Dec 30 of that year. A date
    /// expands anywhere; an age or a parameter needs the plan
    /// ([`RothConversionsParams::for_plan`]), which also supplies the default:
    /// the plan's retirement-age parameter, else its first year.
    #[serde(default)]
    #[ts(optional)]
    pub start: Option<When>,
    /// When conversions stop; defaults to age 73, when RMDs begin, so the
    /// last is on Dec 30 of the year before.
    #[serde(default)]
    #[ts(optional)]
    pub end: Option<When>,
    /// The bank or taxable account that pays the tax; omitted withholds it
    /// from the conversion (less reaches the Roth, and before 59½ the
    /// withheld part pays the early-withdrawal penalty).
    #[serde(default)]
    #[ts(optional)]
    pub pay_tax_from_account_id: Option<RowRef>,
}

/// The age RMDs begin at (SECURE 2.0, born 1951-1959): a template's default
/// end for conversions.
pub const RMD_AGE: u8 = 73;

/// The month and day a conversion template fires. Not Dec 31: the engine
/// captures year-end balances (next year's RMD base and the year-end
/// snapshot) the moment the clock reaches Dec 31, before that day's events,
/// so a Dec 31 conversion would stay in next year's RMD base and show in the
/// balances a year late. `bracket_room` reads the same income on Dec 30.
pub const CONVERSION_DAY: (i8, i8) = (12, 30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TemplateKind {
    Salary,
    EmployerMatch,
    RecurringExpense,
    Retirement,
    HomePurchase,
    SocialSecurity,
    MarketCrash,
    LargeExpense,
    JobLoss,
    RothConversions,
}

/// One template and its parameters, tagged by `kind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum Template {
    Salary(SalaryParams),
    EmployerMatch(EmployerMatchParams),
    RecurringExpense(RecurringExpenseParams),
    Retirement(RetirementParams),
    HomePurchase(HomePurchaseParams),
    SocialSecurity(SocialSecurityParams),
    MarketCrash(MarketCrashParams),
    LargeExpense(LargeExpenseParams),
    JobLoss(JobLossParams),
    RothConversions(RothConversionsParams),
}

impl Template {
    pub fn kind(&self) -> TemplateKind {
        match self {
            Template::Salary(_) => TemplateKind::Salary,
            Template::EmployerMatch(_) => TemplateKind::EmployerMatch,
            Template::RecurringExpense(_) => TemplateKind::RecurringExpense,
            Template::Retirement(_) => TemplateKind::Retirement,
            Template::HomePurchase(_) => TemplateKind::HomePurchase,
            Template::SocialSecurity(_) => TemplateKind::SocialSecurity,
            Template::MarketCrash(_) => TemplateKind::MarketCrash,
            Template::LargeExpense(_) => TemplateKind::LargeExpense,
            Template::JobLoss(_) => TemplateKind::JobLoss,
            Template::RothConversions(_) => TemplateKind::RothConversions,
        }
    }
}

/// What the `expand_template` tool takes: a template and the prefix that
/// namespaces the keys it creates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TemplateRequest {
    #[serde(default)]
    pub key_prefix: String,
    #[serde(flatten)]
    pub template: Template,
}

impl TemplateRequest {
    pub fn expand(&self) -> Result<Expansion, TemplateError> {
        expand_template(&self.key_prefix, &self.template)
    }
}

/// A resource an expansion creates, by the key a later change can point at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreatedKey {
    pub key: String,
    pub kind: RefKind,
}

/// The changes a template lowers to, and the keys they create.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Expansion {
    pub changes: Vec<Change>,
    pub keys: Vec<CreatedKey>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TemplateError(String);

impl TemplateError {
    fn new(message: impl Into<String>) -> Self {
        TemplateError(message.into())
    }
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TemplateError {}

impl From<TemplateError> for PlanError {
    fn from(e: TemplateError) -> Self {
        PlanError::invalid(e.0)
    }
}

// ── expansion ───────────────────────────────────────────────────────────────

/// Lower `template` to changes, creating its resources under
/// `{key_prefix}{local key}`. Parameters are checked here (amounts finite and
/// positive, ages and dates well-formed); references are checked when the
/// changes are resolved against a plan.
pub fn expand_template(key_prefix: &str, template: &Template) -> Result<Expansion, TemplateError> {
    if key_prefix.len() > 64 || key_prefix.chars().any(char::is_control) {
        return Err(TemplateError::new("key_prefix is too long or not text"));
    }
    let mut out = Builder {
        prefix: key_prefix,
        changes: Vec::new(),
        keys: Vec::new(),
    };
    match template {
        Template::Salary(p) => salary(&mut out, p)?,
        Template::EmployerMatch(p) => employer_match(&mut out, p)?,
        Template::RecurringExpense(p) => {
            recurring_expense(&mut out, "expense", p)?;
        }
        Template::Retirement(p) => retirement(&mut out, p)?,
        Template::HomePurchase(p) => home_purchase(&mut out, p)?,
        Template::SocialSecurity(p) => social_security(&mut out, p)?,
        Template::MarketCrash(p) => market_crash(&mut out, p)?,
        Template::LargeExpense(p) => large_expense(&mut out, p)?,
        Template::JobLoss(p) => job_loss(&mut out, p)?,
        Template::RothConversions(p) => roth_conversions(&mut out, p)?,
    }
    Ok(Expansion {
        changes: out.changes,
        keys: out.keys,
    })
}

struct Builder<'a> {
    prefix: &'a str,
    changes: Vec<Change>,
    keys: Vec<CreatedKey>,
}

impl Builder<'_> {
    fn key(&self, local: &str) -> String {
        format!("{}{local}", self.prefix)
    }

    fn create(&mut self, local: &str, kind: RefKind, body: Value) -> RowRef {
        let key = self.key(local);
        let target = match kind {
            RefKind::Event => ChangeTarget::NewEvent(key.clone()),
            RefKind::Asset => ChangeTarget::NewAsset(key.clone()),
            RefKind::Account => ChangeTarget::NewAccount(key.clone()),
            RefKind::Parameter => ChangeTarget::NewParameter(key.clone()),
            RefKind::ReturnProfile => ChangeTarget::NewReturnProfile(key.clone()),
            RefKind::TaxConfig => ChangeTarget::NewTaxConfig(key.clone()),
        };
        self.changes.push(add(target, body));
        self.keys.push(CreatedKey {
            key: key.clone(),
            kind,
        });
        RowRef::new(key)
    }
}

fn add(target: ChangeTarget, body: Value) -> Change {
    Change {
        op: ChangeOp::Add,
        target,
        path: String::new(),
        expect: None,
        value: Some(body),
    }
}

fn positive(what: &str, value: f64) -> Result<(), TemplateError> {
    if value.is_finite() && value > 0. {
        Ok(())
    } else {
        Err(TemplateError::new(format!(
            "{what} must be a positive number"
        )))
    }
}

fn nonnegative(what: &str, value: f64) -> Result<(), TemplateError> {
    if value.is_finite() && value >= 0. {
        Ok(())
    } else {
        Err(TemplateError::new(format!("{what} must not be negative")))
    }
}

fn named(name: &Option<String>, default: &str) -> Result<String, TemplateError> {
    let name = name.as_deref().unwrap_or(default).trim();
    if name.is_empty() || name.len() > 120 {
        return Err(TemplateError::new("a name is 1-120 characters"));
    }
    Ok(name.to_string())
}

fn fixed(value: f64) -> Value {
    json!({"kind": "Fixed", "value": value})
}

fn inflation_adjusted(value: f64) -> Value {
    json!({"kind": "InflationAdjusted", "inner": fixed(value)})
}

/// A dollar amount, inflated unless the caller turned that off.
fn dollars(value: f64, inflate: bool) -> Value {
    if inflate {
        inflation_adjusted(value)
    } else {
        fixed(value)
    }
}

/// `$name`, quoted when the name is not a bare identifier (`$"Monthly spending"`).
fn parameter_ref(name: &str) -> String {
    let mut chars = name.chars();
    let bare = chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_');
    if bare {
        format!("${name}")
    } else {
        format!("$\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn expression(source: String) -> Value {
    json!({"kind": "Expression", "source": source})
}

/// How many times a year something repeating at `interval` happens.
fn per_year(interval: Interval) -> Option<f64> {
    match interval {
        Interval::Never => None,
        Interval::Weekly => Some(52.),
        Interval::BiWeekly => Some(26.),
        Interval::Monthly => Some(12.),
        Interval::Quarterly => Some(4.),
        Interval::Yearly => Some(1.),
    }
}

fn repeating(interval: Interval, start: Option<Value>, end: Option<Value>) -> Value {
    json!({
        "kind": "Repeating",
        "interval": interval,
        "start_condition": start,
        "end_condition": end,
        "max_occurrences": null,
    })
}

fn event_body(
    name: String,
    fires_once: bool,
    trigger: Value,
    effects: Vec<Value>,
) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("name".into(), json!(name));
    body.insert("fires_once".into(), json!(fires_once));
    body.insert("enabled".into(), json!(true));
    body.insert("trigger".into(), trigger);
    body.insert("effects".into(), Value::Array(effects));
    body
}

fn with_sort(mut body: Map<String, Value>, sort_order: Option<i64>) -> Value {
    if let Some(sort_order) = sort_order {
        body.insert("sort_order".into(), json!(sort_order));
    }
    Value::Object(body)
}

fn income(to: &RowRef, amount: Value, tax_free: bool) -> Value {
    json!({
        "kind": "Income",
        "to_account_id": to.json(),
        "amount": amount,
        "amount_mode": "Gross",
        "income_type": if tax_free { "TaxFree" } else { "Taxable" },
    })
}

fn purchases(
    account: &RowRef,
    allocation: &[Allocation],
    annual: f64,
    inflate: bool,
) -> Vec<Value> {
    allocation
        .iter()
        .filter(|a| a.fraction > 0.)
        .map(|a| {
            json!({
                "kind": "AssetPurchase",
                "from_account_id": account.json(),
                "to_account_id": account.json(),
                "asset_id": a.asset_id.json(),
                "amount": dollars(annual * a.fraction, inflate),
            })
        })
        .collect()
}

fn check_allocation(allocation: &[Allocation]) -> Result<(), TemplateError> {
    for a in allocation {
        if !a.fraction.is_finite() || !(0. ..=1.).contains(&a.fraction) {
            return Err(TemplateError::new(
                "allocation fractions are between 0 and 1",
            ));
        }
    }
    if allocation.iter().map(|a| a.fraction).sum::<f64>() > 1. + 1e-9 {
        return Err(TemplateError::new(
            "allocation fractions add up to at most 1",
        ));
    }
    Ok(())
}

fn salary(out: &mut Builder, p: &SalaryParams) -> Result<(), TemplateError> {
    positive("annual_amount", p.annual_amount)?;
    let deferral = match &p.employee_401k {
        Some(d) => {
            nonnegative("the 401(k) deferral", d.annual_amount)?;
            if d.annual_amount > p.annual_amount {
                return Err(TemplateError::new("the 401(k) deferral exceeds the salary"));
            }
            check_allocation(&d.allocation)?;
            Some(d)
        }
        None => None,
    };
    let deferred = deferral.map_or(0., |d| d.annual_amount);
    let mut effects = Vec::new();
    if p.annual_amount - deferred > 0. {
        effects.push(income(
            &p.to_account_id,
            inflation_adjusted(p.annual_amount - deferred),
            false,
        ));
    }
    if let Some(d) = deferral
        && d.annual_amount > 0.
    {
        effects.push(income(
            &d.account_id,
            inflation_adjusted(d.annual_amount),
            true,
        ));
        effects.extend(purchases(
            &d.account_id,
            &d.allocation,
            d.annual_amount,
            true,
        ));
    }
    let start = p.start.as_ref().map(When::trigger).transpose()?;
    let end = p.end.as_ref().map(When::trigger).transpose()?;
    out.create(
        "salary",
        RefKind::Event,
        with_sort(
            event_body(
                named(&p.name, "Salary")?,
                false,
                repeating(Interval::Yearly, start, end),
                effects,
            ),
            p.sort_order,
        ),
    );
    Ok(())
}

fn employer_match(out: &mut Builder, p: &EmployerMatchParams) -> Result<(), TemplateError> {
    if p.salary_parameter.is_none() {
        positive("salary", p.salary)?;
    }
    positive("match_rate", p.match_rate)?;
    if !p.up_to_percent.is_finite()
        || !(0. ..=100.).contains(&p.up_to_percent)
        || p.up_to_percent == 0.
    {
        return Err(TemplateError::new("up_to_percent is between 0 and 100"));
    }
    let employee = p.employee_percent.unwrap_or(p.up_to_percent);
    if !employee.is_finite() || !(0. ..=100.).contains(&employee) {
        return Err(TemplateError::new("employee_percent is between 0 and 100"));
    }
    check_allocation(&p.allocation)?;
    // Dollars matched per dollar of salary.
    let factor = p.match_rate * employee.min(p.up_to_percent) / 100.;
    let scaled = |share: f64| match &p.salary_parameter {
        Some(name) => expression(format!("{} * {}", parameter_ref(name), factor * share)),
        None => json!({
            "kind": "Scale",
            "factor": factor * share,
            "inner": inflation_adjusted(p.salary),
        }),
    };
    let mut effects = vec![income(&p.to_account_id, scaled(1.), true)];
    effects.extend(p.allocation.iter().filter(|a| a.fraction > 0.).map(|a| {
        json!({
            "kind": "AssetPurchase",
            "from_account_id": p.to_account_id.json(),
            "to_account_id": p.to_account_id.json(),
            "asset_id": a.asset_id.json(),
            "amount": scaled(a.fraction),
        })
    }));
    let start = p.start.as_ref().map(When::trigger).transpose()?;
    let end = p.end.as_ref().map(When::trigger).transpose()?;
    out.create(
        "match",
        RefKind::Event,
        with_sort(
            event_body(
                named(&p.name, "Employer 401(k) match")?,
                false,
                repeating(Interval::Yearly, start, end),
                effects,
            ),
            p.sort_order,
        ),
    );
    Ok(())
}

/// The event `local` for spending that repeats. Returns its reference.
fn recurring_expense(
    out: &mut Builder,
    local: &str,
    p: &RecurringExpenseParams,
) -> Result<RowRef, TemplateError> {
    positive("amount", p.amount)?;
    let interval = p.interval.unwrap_or(Interval::Yearly);
    if matches!(interval, Interval::Never) {
        return Err(TemplateError::new("a recurring expense repeats"));
    }
    let inflate = p.inflation_adjusted.unwrap_or(true);
    // The amount as source over the parameter: `$x`, scaled when the parameter
    // counts a different period than the expense repeats over.
    let parameter = match &p.amount_parameter {
        Some(name) => {
            let mut source = parameter_ref(name);
            let own = p.parameter_interval.unwrap_or(interval);
            let (Some(own), Some(paid)) = (per_year(own), per_year(interval)) else {
                return Err(TemplateError::new("a parameter_interval repeats"));
            };
            if own != paid {
                source = format!("{source} * {}", own / paid);
            }
            Some(if inflate {
                format!("inflation({source})")
            } else {
                source
            })
        }
        None => None,
    };
    let mut effects = Vec::new();
    if p.fund_from_investments {
        // The sweep's target is the paying account's cash, so `top_up` is the
        // shortfall the fixed-amount tree spells out.
        let shortfall = match &parameter {
            Some(amount) => expression(format!("top_up({amount})")),
            None => json!({
                "kind": "Max",
                "left": fixed(0.),
                "right": {
                    "kind": "Sub",
                    "left": dollars(p.amount, inflate),
                    "right": {"kind": "AccountCashBalance", "account_id": p.from_account_id.json()},
                },
            }),
        };
        effects.push(json!({
            "kind": "Sweep",
            "to_account_id": p.from_account_id.json(),
            "amount": shortfall,
            "sources": {
                "mode": "Strategy",
                "strategy": "TaxEfficientEarly",
                "exclude_accounts": [],
                "bracket_ceiling": null,
            },
            "amount_mode": "Net",
            "lot_method": "Fifo",
            "income_type": "TaxFree",
        }));
    }
    effects.push(json!({
        "kind": "Expense",
        "from_account_id": p.from_account_id.json(),
        "amount": match parameter {
            Some(amount) => expression(amount),
            None => dollars(p.amount, inflate),
        },
    }));
    let start = p.start.as_ref().map(When::trigger).transpose()?;
    let end = p.end.as_ref().map(When::trigger).transpose()?;
    Ok(out.create(
        local,
        RefKind::Event,
        with_sort(
            event_body(
                named(&Some(p.name.clone()), "")?,
                false,
                repeating(interval, start, end),
                effects,
            ),
            p.sort_order,
        ),
    ))
}

fn retirement(out: &mut Builder, p: &RetirementParams) -> Result<(), TemplateError> {
    let when = p.retirement.trigger()?;
    out.create(
        "retirement",
        RefKind::Event,
        Value::Object(event_body(
            named(&p.name, "Retirement")?,
            true,
            when,
            Vec::new(),
        )),
    );
    if let Some(s) = &p.spending {
        recurring_expense(
            out,
            "retirement_spending",
            &RecurringExpenseParams {
                name: named(&s.name, "Retirement spending")?,
                from_account_id: s.from_account_id.clone(),
                amount: s.annual_amount,
                amount_parameter: None,
                parameter_interval: None,
                interval: None,
                inflation_adjusted: None,
                start: Some(p.retirement.clone()),
                end: None,
                fund_from_investments: s.fund_from_investments,
                sort_order: None,
            },
        )?;
    }
    Ok(())
}

fn home_purchase(out: &mut Builder, p: &HomePurchaseParams) -> Result<(), TemplateError> {
    positive("price", p.price)?;
    nonnegative("down_payment", p.down_payment)?;
    if p.down_payment > p.price {
        return Err(TemplateError::new("the down payment exceeds the price"));
    }
    let name = named(&p.name, "Home")?;
    let inflate = p.inflation_adjusted.unwrap_or(true);
    let financed = p.down_payment < p.price;
    let term = p.term_months.unwrap_or(360);
    if financed {
        let rate = p
            .mortgage_rate
            .ok_or_else(|| TemplateError::new("a financed purchase needs a mortgage_rate"))?;
        if !rate.is_finite() || !(0. ..1.).contains(&rate) {
            return Err(TemplateError::new(
                "mortgage_rate is a fraction, like 0.065",
            ));
        }
        if term == 0 || term > 600 {
            return Err(TemplateError::new("term_months is 1-600"));
        }
    }
    let trigger = p.when.trigger()?;

    let mut asset = json!({"name": format!("{name} (value)"), "initial_price": 100.0});
    if let Some(profile) = &p.appreciation_profile_id {
        asset["return_profile_id"] = profile.json();
    }
    let asset = out.create("home_asset", RefKind::Asset, asset);
    let home = out.create(
        "home",
        RefKind::Account,
        json!({"name": name, "flavor": "Property", "asset_id": asset.json(), "value": 0.0}),
    );
    let mut buy = json!({
        "kind": "BuyProperty",
        "property_account_id": home.json(),
        "from_account_id": p.from_account_id.json(),
        "price": dollars(p.price, inflate),
        "financing": null,
    });
    if financed {
        let mortgage = out.create(
            "mortgage",
            RefKind::Account,
            json!({
                "name": format!("{name} mortgage"),
                "flavor": "Liability",
                "principal": 0.0,
                "interest_rate": p.mortgage_rate,
                "repayment": {"from_account_id": p.from_account_id.json(), "term_months": term},
            }),
        );
        buy["financing"] = json!({
            "loan_account_id": mortgage.json(),
            "down_payment": dollars(p.down_payment, inflate),
            "term_months": term,
        });
    }
    out.create(
        "home_purchase",
        RefKind::Event,
        Value::Object(event_body(
            format!("{name} purchase"),
            true,
            trigger,
            vec![buy],
        )),
    );
    Ok(())
}

fn social_security(out: &mut Builder, p: &SocialSecurityParams) -> Result<(), TemplateError> {
    positive("annual_benefit", p.annual_benefit)?;
    let claim = p.claim.trigger()?;
    out.create(
        "social_security",
        RefKind::Event,
        Value::Object(event_body(
            named(&p.name, "Social Security")?,
            false,
            repeating(Interval::Yearly, Some(claim), None),
            vec![income(
                &p.to_account_id,
                inflation_adjusted(p.annual_benefit),
                false,
            )],
        )),
    );
    Ok(())
}

fn once(name: String, enabled: Option<bool>, when: Value, effects: Vec<Value>) -> Value {
    let mut body = event_body(name, true, when, effects);
    body.insert("enabled".into(), json!(enabled.unwrap_or(true)));
    Value::Object(body)
}

fn roth_conversions(out: &mut Builder, p: &RothConversionsParams) -> Result<(), TemplateError> {
    if !p.ceiling_rate.is_finite() || !(0. ..1.).contains(&p.ceiling_rate) {
        return Err(TemplateError::new(
            "ceiling_rate is a marginal rate between 0 and 1, like 0.22",
        ));
    }
    if p.from_account_id == p.to_account_id {
        return Err(TemplateError::new(
            "a conversion moves money between two accounts",
        ));
    }
    let year = match &p.start {
        Some(When::Date { on_date }) => on_date
            .parse::<jiff::civil::Date>()
            .map_err(|_| TemplateError::new("dates are written YYYY-MM-DD"))?
            .year(),
        _ => {
            return Err(TemplateError::new(
                "start must be a date (the first conversion is that year's Dec 30); \
                 an age or a parameter is resolved against the plan",
            ));
        }
    };
    let end = match &p.end {
        Some(end) => end.trigger()?,
        None => When::age(RMD_AGE).trigger()?,
    };
    let rate = percent_figure(p.ceiling_rate);
    let mut effect = json!({
        "kind": "RothConversion",
        "from_account_id": p.from_account_id.json(),
        "to_account_id": p.to_account_id.json(),
        "amount": expression(format!("bracket_room({rate}%)")),
    });
    if let Some(payer) = &p.pay_tax_from_account_id {
        effect["pay_tax_from_account_id"] = payer.json();
    }
    let mut body = event_body(
        named(&p.name, "Roth conversions")?,
        false,
        repeating(
            Interval::Yearly,
            Some(json!({"kind": "Date", "on_date": format!(
                "{year:04}-{:02}-{:02}",
                CONVERSION_DAY.0, CONVERSION_DAY.1
            )})),
            Some(end),
        ),
        vec![effect],
    );
    body.insert(
        "description".into(),
        json!(format!(
            "Each Dec 30, convert pre-tax money to the Roth up to the top of the \
             {rate}% bracket. Not modeled: IRMAA, ACA credits, the taxable share \
             of Social Security, gains stacking on ordinary income."
        )),
    );
    // No sort order: a new event goes last, after the year's other events.
    out.create("roth_conversions", RefKind::Event, Value::Object(body));
    Ok(())
}

/// A rate as the percent figure an expression writes: 0.22 -> "22", 0.125 ->
/// "12.5".
fn percent_figure(rate: f64) -> String {
    let percent = (rate * 100. * 1e6).round() / 1e6;
    if percent.fract() == 0. {
        format!("{}", percent as i64)
    } else {
        format!("{percent}")
    }
}

impl RothConversionsParams {
    /// Resolve `start` against `graph` to the date the template expands with:
    /// an age (or age parameter) becomes the day it is reached, a date
    /// parameter its date, and none the plan's retirement-age parameter or,
    /// without one, its start date. Conversions then begin where the plan
    /// stands now; moving the retirement age later does not move them.
    pub fn for_plan(mut self, graph: &crate::graph::ScenarioGraph) -> Result<Self, TemplateError> {
        let at_age = |years: i64, months: i64| -> Result<jiff::civil::Date, TemplateError> {
            graph
                .scenario
                .birth_date
                .as_deref()
                .and_then(|d| d.parse::<jiff::civil::Date>().ok())
                .ok_or_else(|| TemplateError::new("an age needs the plan's birth date"))?
                .checked_add(jiff::Span::new().years(years).months(months))
                .map_err(|_| TemplateError::new("the age is out of range"))
        };
        let from_parameter =
            |row: &crate::graph::ParameterRow| -> Result<jiff::civil::Date, TemplateError> {
                match row.kind.as_str() {
                    "Age" => at_age(
                        row.age_years.unwrap_or_default(),
                        row.age_months.unwrap_or_default(),
                    ),
                    "Date" => row
                        .date_value
                        .as_deref()
                        .and_then(|d| d.parse().ok())
                        .ok_or_else(|| TemplateError::new("the parameter has no date")),
                    _ => Err(TemplateError::new("start is an age or date parameter")),
                }
            };
        let date = match &self.start {
            Some(When::Date { .. }) => return Ok(self),
            Some(When::Age { years, months }) => {
                at_age(i64::from(*years), i64::from(months.unwrap_or(0)))?
            }
            Some(When::AgeParameter { parameter_id } | When::DateParameter { parameter_id }) => {
                let RowRef::Id(id) = parameter_id else {
                    return Err(TemplateError::new(
                        "a parameter the same batch creates has no value yet",
                    ));
                };
                let row = graph
                    .parameters
                    .iter()
                    .find(|p| p.id == *id)
                    .ok_or_else(|| TemplateError::new("no such parameter"))?;
                from_parameter(row)?
            }
            None => match graph
                .parameters
                .iter()
                .find(|p| p.kind == "Age" && p.name.to_lowercase().contains("retire"))
            {
                Some(retirement) => from_parameter(retirement)?,
                None => graph
                    .scenario
                    .start_date
                    .parse()
                    .map_err(|_| TemplateError::new("the plan's start date is invalid"))?,
            },
        };
        self.start = Some(When::Date {
            on_date: date.to_string(),
        });
        Ok(self)
    }
}

fn market_crash(out: &mut Builder, p: &MarketCrashParams) -> Result<(), TemplateError> {
    if !p.drop.is_finite() || p.drop <= 0. || p.drop >= 1. {
        return Err(TemplateError::new("drop is a fraction between 0 and 1"));
    }
    out.create(
        "market_crash",
        RefKind::Event,
        once(
            named(&p.name, "Market crash")?,
            p.enabled,
            p.when.trigger()?,
            vec![json!({"kind": "MarketShock", "drop": p.drop})],
        ),
    );
    Ok(())
}

fn large_expense(out: &mut Builder, p: &LargeExpenseParams) -> Result<(), TemplateError> {
    positive("amount", p.amount)?;
    out.create(
        "large_expense",
        RefKind::Event,
        once(
            named(&p.name, "Large expense")?,
            p.enabled,
            p.when.trigger()?,
            vec![json!({
                "kind": "Expense",
                "from_account_id": p.from_account_id.json(),
                "amount": dollars(p.amount, p.inflation_adjusted.unwrap_or(true)),
            })],
        ),
    );
    Ok(())
}

fn job_loss(out: &mut Builder, p: &JobLossParams) -> Result<(), TemplateError> {
    if p.months == 0 || p.months > 240 {
        return Err(TemplateError::new("months is 1-240"));
    }
    let name = named(&p.name, "Job loss")?;
    let start = out.create(
        "job_loss",
        RefKind::Event,
        once(
            name.clone(),
            p.enabled,
            p.when.trigger()?,
            vec![json!({"kind": "PauseEvent", "target_event_id": p.salary_event_id.json()})],
        ),
    );
    out.create(
        "job_loss_recovery",
        RefKind::Event,
        once(
            format!("{name}: back to work"),
            p.enabled,
            json!({
                "kind": "RelativeToEvent",
                "event_id": start.json(),
                "unit": "Months",
                "value": p.months,
            }),
            vec![json!({"kind": "ResumeEvent", "target_event_id": p.salary_event_id.json()})],
        ),
    );
    Ok(())
}

// ── building blocks ─────────────────────────────────────────────────────────

/// A bank account holding `cash`.
pub fn bank_account(
    key: &str,
    name: &str,
    cash: f64,
    return_profile_id: RowRef,
    sort_order: Option<i64>,
) -> Change {
    let mut body = json!({
        "name": name,
        "flavor": "Bank",
        "cash_value": cash,
        "return_profile_id": return_profile_id.json(),
    });
    if let Some(sort_order) = sort_order {
        body["sort_order"] = json!(sort_order);
    }
    add(ChangeTarget::NewAccount(key.into()), body)
}

/// An investment account with no cash and the given opening lots (see
/// [`position`]). `tax_status` is `Taxable`, `TaxDeferred` or `TaxFree`.
pub fn investment_account(
    key: &str,
    name: &str,
    tax_status: &str,
    cash_return_profile_id: RowRef,
    sort_order: Option<i64>,
    positions: Vec<Value>,
) -> Change {
    let mut body = json!({
        "name": name,
        "flavor": "Investment",
        "tax_status": tax_status,
        "cash_value": 0.0,
        "cash_return_profile_id": cash_return_profile_id.json(),
        "positions": positions,
    });
    if let Some(sort_order) = sort_order {
        body["sort_order"] = json!(sort_order);
    }
    add(ChangeTarget::NewAccount(key.into()), body)
}

/// An asset priced at 1, so a position's units are dollars, moving with the
/// given return profile.
pub fn allocation_asset(
    key: &str,
    name: &str,
    return_profile_id: RowRef,
    sort_order: Option<i64>,
) -> Change {
    let mut body = json!({
        "name": name,
        "initial_price": 1.0,
        "return_profile_id": return_profile_id.json(),
    });
    if let Some(sort_order) = sort_order {
        body["sort_order"] = json!(sort_order);
    }
    add(ChangeTarget::NewAsset(key.into()), body)
}

/// A named plan parameter, which amounts can use as `$name` and schedules as
/// an age or date.
pub fn parameter(key: &str, name: &str, value: ParameterValueSpec) -> Change {
    add(
        ChangeTarget::NewParameter(key.into()),
        json!({"name": name, "value": value}),
    )
}

/// An opening lot, bought on the plan's start date.
pub fn position(asset_id: &RowRef, units: f64, cost_basis: f64) -> Value {
    json!({"asset_id": asset_id.json(), "units": units, "cost_basis": cost_basis})
}

#[cfg(test)]
mod tests;
