//! Document tools: deterministic functions over stored documents that the
//! drafting agent calls instead of reading hundreds of transactions or
//! comparing figures by eye. Pure over a [`Document`] (and a plan graph); no
//! I/O, no model. Results are serde/ts-rs types the tool loop can return as
//! JSON and the checker can cite as computed evidence.
//!
//! * [`summarize_transactions`]: monthly totals by category, with outliers.
//! * [`match_account`]: which plan account a statement belongs to.
//! * [`reconcile`]: a statement's balances and positions against the plan's.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use ts_rs::TS;

use super::classify::DocumentKind;
use super::data::{AccountData, Transaction};
use super::store::Document;
use crate::compile::rows::{AccountRow, ScenarioGraph};

fn cents(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

// ── summarize_transactions ──────────────────────────────────────────────────

/// A category's total in a month.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct CategoryTotal {
    pub category: String,
    /// Spending, positive; refunds within the category net against it.
    pub total: f64,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct MonthSummary {
    /// `YYYY-MM`.
    pub month: String,
    pub spending: f64,
    pub income: f64,
    pub by_category: Vec<CategoryTotal>,
    /// The statement does not cover the whole month (it starts or ends
    /// inside it); left out of the averages when full months are available.
    pub partial: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct CategoryAverage {
    pub category: String,
    pub monthly_average: f64,
}

/// A transaction well above what is usual for its category.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct Outlier {
    pub date: String,
    pub description: String,
    pub category: String,
    pub amount: f64,
    /// The category's median transaction.
    pub category_median: f64,
    /// `amount / category_median`.
    pub ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct TransactionSummary {
    pub document_id: i64,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
    pub months: Vec<MonthSummary>,
    /// Over the full months (or all of them when there are too few).
    pub average_monthly_spending: f64,
    pub average_monthly_income: f64,
    pub category_averages: Vec<CategoryAverage>,
    pub outliers: Vec<Outlier>,
    pub transactions_counted: usize,
    /// Transfers and card payments, which are neither spending nor income.
    pub transfers_excluded: usize,
}

/// A transaction is an outlier above this many times its category's median.
pub const OUTLIER_RATIO: f64 = 2.5;
/// ... when the category has at least this many transactions ...
const OUTLIER_MIN_SAMPLE: usize = 3;
/// ... and it is at least this large, so a $9 coffee is never one.
const OUTLIER_MIN_AMOUNT: f64 = 50.0;

const CATEGORIES: &[(&str, &[&str])] = &[
    (
        "transfer",
        &[
            "transfer",
            "xfer",
            "payment thank you",
            "credit card payment",
            "autopay",
            "card payment",
            "zelle",
            "venmo",
            "paypal transfer",
        ],
    ),
    (
        "income",
        &[
            "payroll",
            "direct dep",
            "direct deposit",
            "salary",
            "paycheck",
            "employer",
        ],
    ),
    (
        "interest",
        &["interest paid", "interest earned", "dividend"],
    ),
    (
        "housing",
        &["rent", "mortgage", "hoa", "property tax", "landlord"],
    ),
    (
        "utilities",
        &[
            "electric",
            "utility",
            "utilities",
            "water",
            "sewer",
            "gas co",
            "comcast",
            "xfinity",
            "verizon",
            "at&t",
            "t-mobile",
            "internet",
            "spectrum",
            "trash",
        ],
    ),
    (
        "groceries",
        &[
            "grocery",
            "groceries",
            "kroger",
            "safeway",
            "whole foods",
            "trader joe",
            "aldi",
            "publix",
            "wegmans",
            "costco",
            "sprouts",
            "heb ",
            "supermarket",
        ],
    ),
    (
        "dining",
        &[
            "restaurant",
            "cafe",
            "coffee",
            "starbucks",
            "mcdonald",
            "doordash",
            "ubereats",
            "uber eats",
            "grubhub",
            "pizza",
            "chipotle",
            "taco",
            "bar & grill",
            "diner",
            "bakery",
        ],
    ),
    (
        "travel",
        &[
            "airline",
            "airlines",
            "delta air",
            "united air",
            "southwest",
            "american air",
            "jetblue",
            "hotel",
            "airbnb",
            "marriott",
            "hilton",
            "expedia",
            "hertz",
            "booking.com",
            "flight",
        ],
    ),
    (
        "transportation",
        &[
            "uber",
            "lyft",
            "shell",
            "chevron",
            "exxon",
            "bp ",
            "fuel",
            "gas station",
            "parking",
            "transit",
            "metro",
            "toll",
            "auto parts",
        ],
    ),
    (
        "insurance",
        &[
            "insurance",
            "geico",
            "state farm",
            "allstate",
            "progressive",
        ],
    ),
    (
        "healthcare",
        &[
            "pharmacy",
            "cvs",
            "walgreens",
            "medical",
            "dental",
            "dentist",
            "hospital",
            "clinic",
            "doctor",
            "health",
        ],
    ),
    (
        "subscriptions",
        &[
            "netflix",
            "spotify",
            "hulu",
            "disney",
            "subscription",
            "apple.com/bill",
            "prime video",
            "youtube",
            "membership",
            "gym",
        ],
    ),
    (
        "shopping",
        &[
            "amazon",
            "target",
            "walmart",
            "best buy",
            "home depot",
            "lowe",
            "ikea",
            "etsy",
            "ebay",
        ],
    ),
    ("cash", &["atm", "cash withdrawal"]),
    ("fees", &["fee", "overdraft", "service charge"]),
    ("taxes", &["irs", "tax payment", "franchise tax", "treas"]),
];

/// The category a transaction falls under, by keyword in its description.
pub fn categorize(description: &str, amount: f64) -> &'static str {
    let d = description.to_lowercase();
    for (category, words) in CATEGORIES {
        if words.iter().any(|w| d.contains(w)) {
            return category;
        }
    }
    if amount > 0.0 { "income" } else { "other" }
}

fn month_index(date: &str) -> Option<i64> {
    let y: i64 = date.get(0..4)?.parse().ok()?;
    let m: i64 = date.get(5..7)?.parse().ok()?;
    Some(y * 12 + m - 1)
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.total_cmp(b));
    let n = values.len();
    if n == 0 {
        0.0
    } else if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    }
}

/// Every transaction of the document, across its accounts.
fn transactions(document: &Document) -> Vec<&Transaction> {
    document
        .data
        .accounts
        .iter()
        .flat_map(|a| a.transactions.iter())
        .collect()
}

/// Monthly spending and income by category from the document's transactions,
/// with outliers flagged. `months` keeps only the latest that many calendar
/// months (counted back from the newest transaction).
pub fn summarize_transactions(document: &Document, months: Option<u32>) -> TransactionSummary {
    let mut txns = transactions(document);
    let newest = txns.iter().filter_map(|t| month_index(&t.date)).max();
    if let (Some(newest), Some(n)) = (newest, months.filter(|n| *n > 0)) {
        let cutoff = newest - i64::from(n) + 1;
        txns.retain(|t| month_index(&t.date).is_some_and(|m| m >= cutoff));
    }
    let first_date = txns.iter().map(|t| t.date.clone()).min();
    let last_date = txns.iter().map(|t| t.date.clone()).max();

    struct Month {
        spending: f64,
        income: f64,
        cats: BTreeMap<&'static str, (f64, usize)>,
        first_day: u32,
        last_day: u32,
    }
    let mut by_month: BTreeMap<String, Month> = BTreeMap::new();
    let mut transfers = 0;
    let mut counted = 0;
    let mut debits: HashMap<&'static str, Vec<(&Transaction, f64)>> = HashMap::new();
    for t in &txns {
        let category = categorize(&t.description, t.amount);
        if category == "transfer" {
            transfers += 1;
            continue;
        }
        counted += 1;
        let key = t.date[..7.min(t.date.len())].to_owned();
        let day: u32 = t.date.get(8..10).and_then(|d| d.parse().ok()).unwrap_or(15);
        let m = by_month.entry(key).or_insert(Month {
            spending: 0.0,
            income: 0.0,
            cats: BTreeMap::new(),
            first_day: day,
            last_day: day,
        });
        m.first_day = m.first_day.min(day);
        m.last_day = m.last_day.max(day);
        let income_like = matches!(category, "income" | "interest") && t.amount > 0.0;
        if income_like {
            m.income += t.amount;
            continue;
        }
        // Money out is spending; money back in a spending category is a refund.
        let spent = -t.amount;
        m.spending += spent;
        let cat = m.cats.entry(category).or_insert((0.0, 0));
        cat.0 += spent;
        cat.1 += 1;
        if t.amount < 0.0 {
            debits.entry(category).or_default().push((t, spent));
        }
    }

    let keys: Vec<String> = by_month.keys().cloned().collect();
    let months_out: Vec<MonthSummary> = by_month
        .into_iter()
        .enumerate()
        .map(|(i, (month, m))| {
            let starts_late = i == 0 && m.first_day > 5;
            let ends_early = i == keys.len() - 1 && m.last_day < 25;
            let mut by_category: Vec<CategoryTotal> = m
                .cats
                .into_iter()
                .map(|(category, (total, count))| CategoryTotal {
                    category: category.to_owned(),
                    total: cents(total),
                    count,
                })
                .collect();
            by_category.sort_by(|a, b| b.total.total_cmp(&a.total));
            MonthSummary {
                month,
                spending: cents(m.spending),
                income: cents(m.income),
                by_category,
                partial: starts_late || ends_early,
            }
        })
        .collect();

    // Averages over full months, unless that would leave fewer than two.
    let full = months_out.iter().filter(|m| !m.partial).count();
    let basis: Vec<&MonthSummary> = if full >= 2 {
        months_out.iter().filter(|m| !m.partial).collect()
    } else {
        months_out.iter().collect()
    };
    let n = basis.len().max(1) as f64;
    let average_monthly_spending = cents(basis.iter().map(|m| m.spending).sum::<f64>() / n);
    let average_monthly_income = cents(basis.iter().map(|m| m.income).sum::<f64>() / n);
    let mut totals: BTreeMap<&str, f64> = BTreeMap::new();
    for m in &basis {
        for c in &m.by_category {
            *totals.entry(c.category.as_str()).or_default() += c.total;
        }
    }
    let mut category_averages: Vec<CategoryAverage> = totals
        .into_iter()
        .map(|(category, total)| CategoryAverage {
            category: category.to_owned(),
            monthly_average: cents(total / n),
        })
        .collect();
    category_averages.sort_by(|a, b| b.monthly_average.total_cmp(&a.monthly_average));

    let mut outliers = Vec::new();
    for (category, list) in &debits {
        if list.len() < OUTLIER_MIN_SAMPLE {
            continue;
        }
        let mut amounts: Vec<f64> = list.iter().map(|(_, a)| *a).collect();
        let med = median(&mut amounts);
        if med <= 0.0 {
            continue;
        }
        for (t, spent) in list {
            if *spent > OUTLIER_RATIO * med && *spent >= OUTLIER_MIN_AMOUNT {
                outliers.push(Outlier {
                    date: t.date.clone(),
                    description: t.description.clone(),
                    category: (*category).to_owned(),
                    amount: cents(*spent),
                    category_median: cents(med),
                    ratio: (spent / med * 10.0).round() / 10.0,
                });
            }
        }
    }
    outliers.sort_by(|a, b| b.amount.total_cmp(&a.amount).then(a.date.cmp(&b.date)));

    TransactionSummary {
        document_id: document.id,
        first_date,
        last_date,
        months: months_out,
        average_monthly_spending,
        average_monthly_income,
        category_averages,
        outliers,
        transactions_counted: counted,
        transfers_excluded: transfers,
    }
}

// ── plan accounts, as the matching tools see them ───────────────────────────

/// A plan account's figures for comparison.
struct PlanAccount<'a> {
    row: &'a AccountRow,
    /// `Taxable`, `TaxDeferred` or `TaxFree` for an investment account.
    tax_status: Option<&'a str>,
    value: f64,
    cash: Option<f64>,
    /// Names of the assets it holds.
    holdings: Vec<&'a str>,
}

fn plan_accounts(graph: &ScenarioGraph) -> Vec<PlanAccount<'_>> {
    let price: HashMap<i64, f64> = graph
        .assets
        .iter()
        .map(|a| (a.id, a.initial_price))
        .collect();
    let assets: HashMap<i64, &str> = graph
        .assets
        .iter()
        .map(|a| (a.id, a.name.as_str()))
        .collect();
    graph
        .accounts
        .iter()
        .filter_map(|row| match row.flavor.as_str() {
            "Bank" => {
                let cash = graph.bank.get(&row.id)?.cash_value;
                Some(PlanAccount {
                    row,
                    tax_status: None,
                    value: cash,
                    cash: Some(cash),
                    holdings: Vec::new(),
                })
            }
            "Investment" => {
                let inv = graph.investment.get(&row.id)?;
                let held: f64 = graph
                    .positions
                    .get(&row.id)
                    .into_iter()
                    .flatten()
                    .map(|p| p.units * price.get(&p.asset_id).copied().unwrap_or(0.0))
                    .sum();
                Some(PlanAccount {
                    row,
                    tax_status: Some(inv.tax_status.as_str()),
                    value: inv.cash_value + held,
                    cash: Some(inv.cash_value),
                    holdings: graph
                        .positions
                        .get(&row.id)
                        .into_iter()
                        .flatten()
                        .filter_map(|p| assets.get(&p.asset_id).copied())
                        .collect(),
                })
            }
            "Liability" => Some(PlanAccount {
                row,
                tax_status: None,
                value: graph.liability.get(&row.id)?.principal,
                cash: None,
                holdings: Vec::new(),
            }),
            _ => None,
        })
        .collect()
}

/// Lowercase alphanumeric words.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Words too generic to identify an institution.
const GENERIC: &[&str] = &[
    "account",
    "accounts",
    "checking",
    "savings",
    "bank",
    "brokerage",
    "taxable",
    "joint",
    "ira",
    "roth",
    "401k",
    "401",
    "k",
    "403b",
    "plan",
    "retirement",
    "investment",
    "investments",
    "the",
    "my",
    "and",
    "of",
    "cash",
    "money",
    "market",
    "fund",
    "trust",
    "credit",
    "card",
    "loan",
    "mortgage",
    "traditional",
    "individual",
];

// ── match_account ───────────────────────────────────────────────────────────

/// A plan account a statement might belong to.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct AccountCandidate {
    pub account_id: i64,
    pub name: String,
    /// 0 to 1.
    pub score: f64,
    pub reasons: Vec<String>,
    pub plan_value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct DocumentAccountMatch {
    /// Index into the document's accounts (a statement may hold several).
    pub document_account: usize,
    pub last4: Option<String>,
    pub balance: Option<f64>,
    /// Best first.
    pub candidates: Vec<AccountCandidate>,
    /// The candidate to use without asking: it scores well and clearly beats
    /// the next. `None` means ask ("Which account?") or add a new one.
    pub best_account_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct AccountMatches {
    pub document_id: i64,
    pub institution: Option<String>,
    pub accounts: Vec<DocumentAccountMatch>,
}

/// Candidates below this score are not listed.
const MIN_CANDIDATE_SCORE: f64 = 0.2;
/// The top candidate is `best` at this score, when it also leads by [`LEAD`].
const BEST_SCORE: f64 = 0.5;
const LEAD: f64 = 0.15;

fn score_account(
    document: &Document,
    doc_account: &AccountData,
    plan: &PlanAccount<'_>,
    first_page: &str,
) -> (f64, Vec<String>) {
    let mut score: f64 = 0.0;
    let mut reasons = Vec::new();
    let name_text = format!(
        "{} {}",
        plan.row.name,
        plan.row.description.as_deref().unwrap_or("")
    );
    let name_words = words(&name_text);
    let distinctive: Vec<&String> = name_words
        .iter()
        .filter(|w| {
            w.len() >= 3 && !GENERIC.contains(&w.as_str()) && !w.chars().all(|c| c.is_ascii_digit())
        })
        .collect();

    // Institution: the statement's, found in the account's name; failing that,
    // the account's own distinctive words found in the statement's first page.
    if let Some(inst) = &document.data.institution {
        let inst_words: Vec<String> = words(inst)
            .into_iter()
            .filter(|w| !GENERIC.contains(&w.as_str()))
            .collect();
        if !inst_words.is_empty() && inst_words.iter().all(|w| name_words.contains(w)) {
            score += 0.4;
            reasons.push(format!("institution {inst} is in the account name"));
        }
    }
    if reasons.is_empty() && !distinctive.is_empty() {
        let page = first_page.to_lowercase();
        let found: Vec<&&String> = distinctive
            .iter()
            .filter(|w| page.contains(w.as_str()))
            .collect();
        if found.len() == distinctive.len() {
            score += 0.3;
            reasons.push("the account's name appears in the statement".into());
        }
    }

    if let Some(last4) = &doc_account.last4
        && name_words
            .iter()
            .any(|w| w == last4 || w.ends_with(last4.as_str()) && w.len() <= 8)
    {
        score += 0.4;
        reasons.push(format!("last four digits {last4} are in the account name"));
    }

    if let Some(balance) = doc_account.headline_balance() {
        let base = balance.abs().max(plan.value.abs()).max(1.0);
        let rel = (balance.abs() - plan.value.abs()).abs() / base;
        let (points, label) = if rel <= 0.005 {
            (0.3, "within 0.5%")
        } else if rel <= 0.02 {
            (0.2, "within 2%")
        } else if rel <= 0.10 {
            (0.1, "within 10%")
        } else {
            (0.0, "")
        };
        if points > 0.0 {
            score += points;
            reasons.push(format!("balance is {label} of the plan's"));
        }
    }

    // Holdings: the statement's positions are held in the account.
    if !doc_account.positions.is_empty() {
        let shared = doc_account
            .positions
            .iter()
            .filter(|p| {
                plan.holdings
                    .iter()
                    .any(|name| asset_matches(p.symbol.as_deref(), p.name.as_deref(), name))
            })
            .count();
        if shared * 2 >= doc_account.positions.len() && shared > 0 {
            score += 0.3;
            reasons.push(format!(
                "{shared} of {} holdings are held in the account",
                doc_account.positions.len()
            ));
        }
    }

    let flavor_fits = matches!(
        (document.kind, plan.row.flavor.as_str(), plan.tax_status),
        (
            DocumentKind::BankStatement | DocumentKind::Transactions,
            "Bank",
            _
        ) | (
            DocumentKind::BrokerageStatement,
            "Investment",
            Some("Taxable")
        ) | (
            DocumentKind::RetirementStatement,
            "Investment",
            Some("TaxDeferred" | "TaxFree")
        )
    );
    if flavor_fits {
        score += 0.1;
        reasons.push("the kind of account fits the kind of document".into());
    }
    if let Some(kind) = &doc_account.kind
        && name_words.iter().any(|w| w == kind)
    {
        score += 0.1;
        reasons.push(format!("both are {kind} accounts"));
    }
    ((score.min(1.0) * 100.0).round() / 100.0, reasons)
}

/// Which of the plan's accounts each account in `document` most likely is:
/// candidates by institution, last four digits and balance proximity, with
/// scores and the reasons for them.
pub fn match_account(document: &Document, graph: &ScenarioGraph) -> AccountMatches {
    let plan = plan_accounts(graph);
    let first_page = document
        .text
        .split(super::store::PAGE_BREAK)
        .next()
        .unwrap_or("");
    let accounts = document
        .data
        .accounts
        .iter()
        .enumerate()
        .map(|(i, doc_account)| {
            let mut candidates: Vec<AccountCandidate> = plan
                .iter()
                .filter_map(|p| {
                    let (score, reasons) = score_account(document, doc_account, p, first_page);
                    (score >= MIN_CANDIDATE_SCORE).then(|| AccountCandidate {
                        account_id: p.row.id,
                        name: p.row.name.clone(),
                        score,
                        reasons,
                        plan_value: cents(p.value),
                    })
                })
                .collect();
            candidates.sort_by(|a, b| {
                b.score
                    .total_cmp(&a.score)
                    .then(a.account_id.cmp(&b.account_id))
            });
            candidates.truncate(5);
            let best_account_id = match candidates.as_slice() {
                [only] if only.score >= BEST_SCORE => Some(only.account_id),
                [top, next, ..] if top.score >= BEST_SCORE && top.score - next.score >= LEAD => {
                    Some(top.account_id)
                }
                _ => None,
            };
            DocumentAccountMatch {
                document_account: i,
                last4: doc_account.last4.clone(),
                balance: doc_account.headline_balance().map(cents),
                candidates,
                best_account_id,
            }
        })
        .collect();
    AccountMatches {
        document_id: document.id,
        institution: document.data.institution.clone(),
        accounts,
    }
}

// ── reconcile ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct BalanceDiff {
    pub document: f64,
    pub plan: f64,
    /// `document - plan`.
    pub difference: f64,
    /// `difference` as a percent of the plan's figure (`None` at zero).
    pub percent: Option<f64>,
    /// Worth a "Fix · Balance" note: beyond $50 and 0.5%.
    pub significant: bool,
    pub as_of: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PositionStatus {
    Matches,
    UnitsDiffer,
    /// In the statement, not in the plan's account.
    MissingInPlan,
    /// In the plan's account, not in the statement.
    MissingInDocument,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct PositionDiff {
    pub symbol: Option<String>,
    pub name: Option<String>,
    /// The plan asset it was matched to.
    pub asset_id: Option<i64>,
    pub document_units: Option<f64>,
    pub plan_units: Option<f64>,
    pub units_difference: Option<f64>,
    pub document_value: Option<f64>,
    pub plan_value: Option<f64>,
    pub status: PositionStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct AccountReconciliation {
    pub document_account: usize,
    pub last4: Option<String>,
    /// The plan account it was matched to; `None` when nothing matched
    /// well enough (a new account to add).
    pub account_id: Option<i64>,
    pub account_name: Option<String>,
    pub match_score: Option<f64>,
    pub balance: Option<BalanceDiff>,
    /// The cash balance against the plan account's cash.
    pub cash: Option<BalanceDiff>,
    pub positions: Vec<PositionDiff>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct Reconciliation {
    pub document_id: i64,
    pub accounts: Vec<AccountReconciliation>,
}

fn balance_diff(document: f64, plan: f64, as_of: Option<String>) -> BalanceDiff {
    let difference = document - plan;
    BalanceDiff {
        document: cents(document),
        plan: cents(plan),
        difference: cents(difference),
        percent: (plan.abs() > f64::EPSILON).then(|| (difference / plan * 1000.0).round() / 10.0),
        significant: difference.abs() > 50.0_f64.max(plan.abs() * 0.005),
        as_of,
    }
}

/// Whether a statement position is the plan asset `name`.
fn asset_matches(symbol: Option<&str>, doc_name: Option<&str>, asset_name: &str) -> bool {
    let asset = asset_name.to_lowercase();
    let asset_words = words(&asset);
    if let Some(s) = symbol {
        let s = s.to_lowercase();
        if asset == s || asset_words.contains(&s) {
            return true;
        }
    }
    doc_name.is_some_and(|n| {
        let n = n.to_lowercase();
        !n.is_empty() && (n == asset || n.contains(&asset) || asset.contains(&n))
    })
}

/// A structured diff of the document's figures against the plan's: for each
/// account in the document, the best-matching plan account, the balance
/// difference, and position-by-position share counts. Accounts with no
/// confident match come back with `account_id: None`.
pub fn reconcile(document: &Document, graph: &ScenarioGraph) -> Reconciliation {
    let matches = match_account(document, graph);
    let plan = plan_accounts(graph);
    let asset_by_id: HashMap<i64, (&str, f64)> = graph
        .assets
        .iter()
        .map(|a| (a.id, (a.name.as_str(), a.initial_price)))
        .collect();

    let accounts = document
        .data
        .accounts
        .iter()
        .zip(&matches.accounts)
        .map(|(doc_account, m)| {
            let chosen = m.best_account_id.or({
                // A lone candidate that is merely plausible is still the only one.
                match m.candidates.as_slice() {
                    [only] if only.score >= 0.4 => Some(only.account_id),
                    _ => None,
                }
            });
            let Some(plan_account) = chosen.and_then(|id| plan.iter().find(|p| p.row.id == id))
            else {
                return AccountReconciliation {
                    document_account: m.document_account,
                    last4: m.last4.clone(),
                    account_id: None,
                    account_name: None,
                    match_score: m.candidates.first().map(|c| c.score),
                    balance: None,
                    cash: None,
                    positions: Vec::new(),
                };
            };
            let as_of = doc_account.balances.iter().find_map(|b| b.as_of.clone());
            let balance = doc_account
                .headline_balance()
                .map(|d| balance_diff(d, plan_account.value, as_of.clone()));
            let cash = match (doc_account.cash_balance(), plan_account.cash) {
                (Some(d), Some(p)) if !doc_account.positions.is_empty() => {
                    Some(balance_diff(d, p, as_of))
                }
                _ => None,
            };

            let mut positions = Vec::new();
            if !doc_account.positions.is_empty() && plan_account.row.flavor == "Investment" {
                // The plan's holdings by asset, summed over lots.
                let mut held: BTreeMap<i64, f64> = BTreeMap::new();
                for lot in graph
                    .positions
                    .get(&plan_account.row.id)
                    .into_iter()
                    .flatten()
                {
                    *held.entry(lot.asset_id).or_default() += lot.units;
                }
                let mut seen: Vec<i64> = Vec::new();
                for p in &doc_account.positions {
                    let asset = asset_by_id.iter().find(|(_, (name, _))| {
                        asset_matches(p.symbol.as_deref(), p.name.as_deref(), name)
                    });
                    let (asset_id, plan_units, plan_value) = match asset {
                        Some((id, (_, price))) => {
                            let units = held.get(id).copied();
                            seen.push(*id);
                            (Some(*id), units, units.map(|u| u * price))
                        }
                        None => (None, None, None),
                    };
                    let status = match plan_units {
                        None => PositionStatus::MissingInPlan,
                        Some(u)
                            if (u - p.units).abs()
                                <= 0.005 * u.abs().max(p.units.abs()).max(1e-9) =>
                        {
                            PositionStatus::Matches
                        }
                        Some(_) => PositionStatus::UnitsDiffer,
                    };
                    positions.push(PositionDiff {
                        symbol: p.symbol.clone(),
                        name: p.name.clone(),
                        asset_id,
                        document_units: Some(p.units),
                        plan_units,
                        units_difference: plan_units.map(|u| ((p.units - u) * 1e6).round() / 1e6),
                        document_value: p.market_value.map(cents),
                        plan_value: plan_value.map(cents),
                        status,
                    });
                }
                for (asset_id, units) in &held {
                    if seen.contains(asset_id) {
                        continue;
                    }
                    let (name, price) = asset_by_id.get(asset_id).copied().unwrap_or(("", 0.0));
                    positions.push(PositionDiff {
                        symbol: None,
                        name: Some(name.to_owned()),
                        asset_id: Some(*asset_id),
                        document_units: None,
                        plan_units: Some(*units),
                        units_difference: None,
                        document_value: None,
                        plan_value: Some(cents(units * price)),
                        status: PositionStatus::MissingInDocument,
                    });
                }
            }
            AccountReconciliation {
                document_account: m.document_account,
                last4: m.last4.clone(),
                account_id: Some(plan_account.row.id),
                account_name: Some(plan_account.row.name.clone()),
                match_score: m.candidates.first().map(|c| c.score),
                balance,
                cash,
                positions,
            }
        })
        .collect();
    Reconciliation {
        document_id: document.id,
        accounts,
    }
}
