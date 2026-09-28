//! The per-review half of the prompt: one plan and one run, rendered for a
//! model to read, plus what the loop needs to check what the model sends back.
//!
//! Pure. Bodies appear exactly as their GET routes return them (via
//! `suggest::read`), because those are the shapes `Change` paths point into;
//! everything else is worked out here so the model does not have to: lot
//! values, gains, account totals, ages, and names next to every id.

use std::collections::{BTreeSet, HashSet};
use std::fmt::Write as _;

use serde_json::Value;

use crate::api::runs::Results;
use crate::compile::rows::{DistributionRow, ScenarioGraph};
use crate::suggest::read;
use crate::suggest::rules::{Draft, Kind};
use crate::suggest::{Change, ChangeTarget};

/// Everything one review sends the model, and what it checks replies against.
#[derive(Debug, Clone)]
pub struct ReviewContext {
    pub run_id: i64,
    /// The plan, the run and the existing notes, as the first user turn.
    pub text: String,
    pub(super) events: HashSet<i64>,
    pub(super) accounts: HashSet<i64>,
    /// First and last calendar year the run's cash flows cover.
    pub(super) years: Option<(i64, i64)>,
    /// Dates of the shown path's balance points.
    pub(super) dates: HashSet<String>,
    /// Notes already written, for the duplicate check.
    pub(super) existing: Vec<Existing>,
}

/// A note already on the board, reduced to what makes two notes the same.
#[derive(Debug, Clone)]
pub(super) struct Existing {
    pub kind: Kind,
    pub title: String,
    pub edits: BTreeSet<String>,
}

impl Existing {
    pub fn new(kind: Kind, title: &str, changes: &[Change]) -> Self {
        Self {
            kind,
            title: normalize(title),
            edits: edits(changes),
        }
    }
}

/// Lowercase, alphanumeric words only: "Raise USAA's floor!" == "raise usaa s floor".
pub(super) fn normalize(title: &str) -> String {
    title
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Which resource fields a batch of changes touches.
pub(super) fn edits(changes: &[Change]) -> BTreeSet<String> {
    changes
        .iter()
        .map(|c| {
            // A resource the batch creates is named by its kind alone: its key
            // is the author's label, not what the note is about.
            let target = match &c.target {
                ChangeTarget::Event(id) => format!("event:{id}"),
                ChangeTarget::Asset(id) => format!("asset:{id}"),
                ChangeTarget::Account(id) => format!("account:{id}"),
                ChangeTarget::NewEvent(_) => "new_event".to_owned(),
                ChangeTarget::NewAsset(_) => "new_asset".to_owned(),
                ChangeTarget::NewAccount(_) => "new_account".to_owned(),
            };
            format!("{target}{}", c.path)
        })
        .collect()
}

impl ReviewContext {
    /// `rules` are the notes FinPlan's rules already wrote for this run; the
    /// model is shown them and its notes are checked against them.
    pub fn build(graph: &ScenarioGraph, results: &Results, rules: &[Draft]) -> Self {
        let mut text = String::new();
        render_plan(&mut text, graph);
        render_run(&mut text, graph, results);
        render_existing(&mut text, rules);

        let dates = results
            .bands
            .iter()
            .find(|b| b.path_id == results.series_id)
            .or(results.bands.first())
            .map(|b| b.dates.iter().cloned().collect())
            .unwrap_or_default();
        let years = results
            .cash_flows
            .iter()
            .map(|c| c.year)
            .min()
            .zip(results.cash_flows.iter().map(|c| c.year).max());

        Self {
            run_id: results.run_id,
            text,
            events: graph.events.iter().map(|e| e.id).collect(),
            accounts: graph.accounts.iter().map(|a| a.id).collect(),
            years,
            dates,
            existing: rules
                .iter()
                .map(|d| {
                    let changes: Vec<Change> =
                        d.paths.iter().flat_map(|p| p.changes().cloned()).collect();
                    Existing::new(d.kind, &d.title, &changes)
                })
                .collect(),
        }
    }
}

// ── formatting ──────────────────────────────────────────────────────────────

fn money(v: f64) -> String {
    let rounded = v.round();
    let sign = if rounded < 0.0 { "-" } else { "" };
    let digits = format!("{:.0}", rounded.abs());
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{sign}${out}")
}

/// A unit price: cents matter.
fn unit_price(v: f64) -> String {
    let whole = money(v.trunc());
    format!(
        "{whole}.{:02}",
        ((v.abs().fract() * 100.0).round() as i64).min(99)
    )
}

fn pct(v: f64) -> String {
    format!("{:.2}%", v * 100.0)
}

fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn year_of(date: &str) -> Option<i64> {
    date.get(0..4)?.parse().ok()
}

fn distribution(graph: &ScenarioGraph, id: i64, depth: usize) -> String {
    let Some(d) = graph.distributions.get(&id) else {
        return format!("distribution #{id}");
    };
    describe(graph, d, depth)
}

fn describe(graph: &ScenarioGraph, d: &DistributionRow, depth: usize) -> String {
    match d.kind.as_str() {
        "Fixed" => format!("Fixed {}", d.rate.map(pct).unwrap_or_default()),
        "Normal" | "LogNormal" => format!(
            "{} mean {}, sd {}",
            d.kind,
            d.mean.map(pct).unwrap_or_default(),
            d.std_dev.map(pct).unwrap_or_default()
        ),
        "Bootstrap" => format!(
            "Bootstrap of historical {} returns, block {} years",
            d.history_preset.as_deref().unwrap_or("?"),
            d.block_size.unwrap_or(1)
        ),
        "None" => "no growth (holds nominal value)".to_owned(),
        "RegimeSwitching" if depth < 2 => format!(
            "Regime switching: bull {} / bear {}, P(bull->bear) {}, P(bear->bull) {}",
            d.bull_id
                .map(|id| distribution(graph, id, depth + 1))
                .unwrap_or_default(),
            d.bear_id
                .map(|id| distribution(graph, id, depth + 1))
                .unwrap_or_default(),
            d.bull_to_bear_prob.map(pct).unwrap_or_default(),
            d.bear_to_bull_prob.map(pct).unwrap_or_default()
        ),
        other => {
            let mut parts = vec![other.to_owned()];
            for (name, value) in [
                ("mean", d.mean),
                ("sd", d.std_dev),
                ("scale", d.scale),
                ("df", d.df),
                ("rate", d.rate),
            ] {
                if let Some(v) = value {
                    parts.push(format!("{name} {v}"));
                }
            }
            parts.join(" ")
        }
    }
}

// ── the plan ────────────────────────────────────────────────────────────────

fn render_plan(out: &mut String, graph: &ScenarioGraph) {
    let s = &graph.scenario;
    let birth_year = s.birth_date.as_deref().and_then(year_of);
    let start_year = year_of(&s.start_date);
    let _ = writeln!(out, "<plan>");
    let _ = writeln!(
        out,
        "Scenario \"{}\": starts {}, runs {} years.{}",
        s.name,
        s.start_date,
        s.duration_years,
        match (birth_year, start_year) {
            (Some(b), Some(y)) => format!(
                " Born {} (age {} at the start, {} at the end).",
                s.birth_date.as_deref().unwrap_or(""),
                y - b,
                y - b + s.duration_years
            ),
            _ => String::new(),
        }
    );

    match (
        &graph.inflation_profile_name,
        graph.inflation_distribution_id,
    ) {
        (name, Some(id)) => {
            let _ = writeln!(
                out,
                "Inflation: {} ({}).",
                name.as_deref().unwrap_or("profile"),
                distribution(graph, id, 0)
            );
        }
        _ => {
            let _ = writeln!(out, "Inflation: none (amounts do not inflate).");
        }
    }

    if let Some(tax) = &graph.tax_config {
        let brackets: Vec<String> = graph
            .tax_brackets
            .iter()
            .map(|b| format!("{} from {}", pct(b.rate), money(b.threshold)))
            .collect();
        let _ = writeln!(
            out,
            "Taxes: {}. Federal brackets: {}. State {}. Long-term capital gains {}. Early-withdrawal penalty {}.",
            tax.name,
            brackets.join(", "),
            pct(tax.state_rate),
            pct(tax.capital_gains_rate),
            pct(tax.early_withdrawal_penalty_rate)
        );
    } else {
        let _ = writeln!(out, "Taxes: none configured.");
    }

    let _ = writeln!(out, "\nReturn profiles in use:");
    let mut profiles: Vec<_> = graph.return_profiles.values().collect();
    profiles.sort_by_key(|p| p.id);
    for p in profiles {
        let users: Vec<&str> = graph
            .assets
            .iter()
            .filter(|a| a.return_profile_id == Some(p.id))
            .map(|a| a.name.as_str())
            .collect();
        let _ = writeln!(
            out,
            "- #{} {}{}: {}. Assets: {}.",
            p.id,
            p.name,
            p.asset_class
                .as_deref()
                .map(|c| format!(" [{c}]"))
                .unwrap_or_default(),
            distribution(graph, p.distribution_id, 0),
            if users.is_empty() {
                "none (cash only)".to_owned()
            } else {
                users.join(", ")
            }
        );
    }

    let _ = writeln!(
        out,
        "\nAssets (body as GET /assets returns it; price is the price at the plan start):"
    );
    let mut assets: Vec<_> = graph.assets.iter().collect();
    assets.sort_by_key(|a| (a.sort_order, a.id));
    for a in assets {
        if let Some(body) = read::asset(graph, a.id) {
            let _ = writeln!(out, "- {}", compact(&body));
        }
    }

    let _ = writeln!(
        out,
        "\nAccounts (body as GET /accounts returns it, then values worked out at the plan start):"
    );
    let price = |asset_id: i64| {
        graph
            .assets
            .iter()
            .find(|a| a.id == asset_id)
            .map(|a| (a.name.as_str(), a.initial_price))
    };
    let mut accounts: Vec<_> = graph.accounts.iter().collect();
    accounts.sort_by_key(|a| (a.sort_order, a.id));
    for a in accounts {
        let Some(body) = read::account(graph, a.id) else {
            continue;
        };
        let _ = writeln!(out, "- #{} {}: {}", a.id, a.name, compact(&body));
        let lots = graph.positions.get(&a.id).map(Vec::as_slice).unwrap_or(&[]);
        let mut held = 0.0;
        let mut basis = 0.0;
        for lot in lots {
            let (name, unit) = price(lot.asset_id).unwrap_or(("?", 0.0));
            let value = lot.units * unit;
            held += value;
            basis += lot.cost_basis;
            let _ = writeln!(
                out,
                "    lot #{}: {} {:.4} units x {} = {}; basis {}; gain {}; bought {}",
                lot.id,
                name,
                lot.units,
                unit_price(unit),
                money(value),
                money(lot.cost_basis),
                money(value - lot.cost_basis),
                lot.purchase_date
            );
        }
        let summary = match a.flavor.as_str() {
            "Investment" => graph.investment.get(&a.id).map(|inv| {
                format!(
                    "{} {}: cash {} + holdings {} = {}; basis {}, unrealized gain {}.",
                    inv.tax_status,
                    a.flavor,
                    money(inv.cash_value),
                    money(held),
                    money(inv.cash_value + held),
                    money(basis),
                    money(held - basis)
                )
            }),
            "Bank" => graph
                .bank
                .get(&a.id)
                .map(|b| format!("Bank: cash {}.", money(b.cash_value))),
            "Liability" => graph.liability.get(&a.id).map(|l| {
                format!(
                    "Liability: principal {} at {} interest{}.",
                    money(l.principal),
                    pct(l.interest_rate),
                    l.term_months
                        .map(|m| format!(", {m}-month term"))
                        .unwrap_or_else(|| ", no repayment term (paid by events)".into())
                )
            }),
            "Property" => graph.property.get(&a.id).map(|p| {
                format!(
                    "Property: {} valued {}.",
                    price(p.asset_id).map(|(n, _)| n).unwrap_or("?"),
                    money(p.value)
                )
            }),
            _ => None,
        };
        if let Some(summary) = summary {
            let _ = writeln!(out, "    {summary}");
        }
    }

    let _ = writeln!(
        out,
        "\nEvents (body as GET /events returns it; ids refer to the accounts, assets and events above):"
    );
    let mut events: Vec<_> = graph.events.iter().collect();
    events.sort_by_key(|e| (e.sort_order, e.id));
    for e in events {
        if let Some(body) = read::event(graph, e.id) {
            let _ = writeln!(out, "- {}", compact(&body));
        }
    }
    let _ = writeln!(out, "</plan>");
}

// ── the run ─────────────────────────────────────────────────────────────────

fn render_run(out: &mut String, graph: &ScenarioGraph, results: &Results) {
    let account_name = |id: Option<i64>| {
        id.and_then(|id| graph.accounts.iter().find(|a| a.id == id))
            .map(|a| format!("{} (#{})", a.name, a.id))
            .unwrap_or_else(|| "an unnamed account".into())
    };
    let event_name = |id: Option<i64>| {
        id.and_then(|id| graph.events.iter().find(|e| e.id == id))
            .map(|e| format!("{} (#{})", e.name, e.id))
            .unwrap_or_else(|| "an unnamed event".into())
    };
    let birth_year = graph.scenario.birth_date.as_deref().and_then(year_of);
    let age = |year: i64| {
        birth_year
            .map(|b| format!("{}", year - b))
            .unwrap_or_else(|| "-".into())
    };

    let st = &results.stats;
    let _ = writeln!(out, "\n<run id=\"{}\">", results.run_id);
    let _ = writeln!(
        out,
        "{} iterations. Success rate {}. Funding success rate {}.",
        st.num_iterations,
        pct(st.success_rate),
        st.funding_success_rate
            .map(pct)
            .unwrap_or_else(|| "not measured".into())
    );
    if !st.percentile_values.is_empty() {
        let values: Vec<String> = st
            .percentile_values
            .iter()
            .map(|p| format!("P{:.0} {}", p.percentile * 100.0, money(p.final_net_worth)))
            .collect();
        let _ = writeln!(out, "Final net worth, nominal: {}.", values.join(", "));
    }
    if let Some(last) = results
        .real_net_worth
        .as_ref()
        .and_then(|r| r.points.last())
    {
        let opt = |v: Option<f64>| v.map(money).unwrap_or_else(|| "-".into());
        let _ = writeln!(
            out,
            "Final net worth in today's dollars (all iterations, at {}): P5 {}, P10 {}, P25 {}, P50 {}, P75 {}, P90 {}, P95 {}.",
            last.date,
            money(last.p5),
            opt(last.p10),
            opt(last.p25),
            money(last.p50),
            opt(last.p75),
            opt(last.p90),
            money(last.p95)
        );
    }

    match &results.funding_diagnostics {
        Some(f) => {
            let _ = writeln!(
                out,
                "Funding diagnostics (all iterations): {} of {} failed the funding check: {} with a cash shortfall, {} with an event failure, {} hit the iteration limit; {} failed while finishing with positive net worth.",
                f.failed,
                f.iterations,
                f.cash_shortfall,
                f.event_failure,
                f.iteration_limit,
                f.failed_solvent
            );
            if !f.shortfall_accounts.is_empty() {
                let accounts: Vec<String> = f
                    .shortfall_accounts
                    .iter()
                    .map(|a| format!("{} {}", account_name(a.account_id), a.count))
                    .collect();
                let _ = writeln!(
                    out,
                    "  Account most overdrawn at the first shortfall: {}.",
                    accounts.join(", ")
                );
            }
            if !f.first_shortfall_years.is_empty() {
                let years: Vec<String> = f
                    .first_shortfall_years
                    .iter()
                    .map(|y| format!("{} (age {}): {}", y.year, age(y.year), y.count))
                    .collect();
                let _ = writeln!(
                    out,
                    "  Year of first shortfall: {}. Median {}.",
                    years.join(", "),
                    f.median_first_shortfall_year
                        .map(|y| format!("{y} (age {})", age(y)))
                        .unwrap_or_else(|| "-".into())
                );
            }
            if !f.liquid_depleted_years.is_empty() {
                let years: Vec<String> = f
                    .liquid_depleted_years
                    .iter()
                    .map(|y| format!("{}: {}", y.year, y.count))
                    .collect();
                let _ = writeln!(out, "  Year liquid balances ran out: {}.", years.join(", "));
            }
            if !f.event_failures.is_empty() {
                let events: Vec<String> = f
                    .event_failures
                    .iter()
                    .map(|e| format!("{} {}", event_name(e.event_id), e.count))
                    .collect();
                let _ = writeln!(
                    out,
                    "  Iterations where an event failed: {}.",
                    events.join(", ")
                );
            }
            if let Some(d) = f.median_max_deficit {
                let _ = writeln!(
                    out,
                    "  Median largest deficit {} (nominal); median years spent short {}.",
                    money(d),
                    f.median_shortfall_years
                        .map(|y| format!("{y:.1}"))
                        .unwrap_or_else(|| "-".into())
                );
            }
        }
        None => {
            let _ = writeln!(out, "Funding diagnostics: not measured for this run.");
        }
    }

    let path = results
        .series_percentile
        .map(|p| format!("the P{:.0} path by final nominal net worth", p * 100.0))
        .unwrap_or_else(|| format!("path {}", results.series_id));
    if !results.warnings.is_empty() {
        let _ = writeln!(
            out,
            "Warnings on {path}: {}.",
            results
                .warnings
                .iter()
                .take(10)
                .map(|w| format!(
                    "{} {}{}",
                    w.kind,
                    w.date.as_deref().unwrap_or(""),
                    w.event_id
                        .map(|id| format!(" ({})", event_name(Some(id))))
                        .unwrap_or_default()
                ))
                .collect::<Vec<_>>()
                .join("; ")
        );
    }

    let _ = writeln!(out, "\nYearly cash flows on {path} (nominal dollars):");
    let _ = writeln!(
        out,
        "year,age,income,expenses,contributions,withdrawals,taxes,appreciation,net_cash_flow"
    );
    for c in &results.cash_flows {
        let _ = writeln!(
            out,
            "{},{},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0}",
            c.year,
            age(c.year),
            c.income,
            c.expenses,
            c.contributions,
            c.withdrawals,
            c.taxes,
            c.appreciation,
            c.net_cash_flow
        );
    }

    if let Some(band) = results
        .bands
        .iter()
        .find(|b| b.path_id == results.series_id)
        .or(results.bands.first())
    {
        let last = band.dates.len().saturating_sub(1);
        let rows: Vec<usize> = (0..band.dates.len())
            .filter(|&i| {
                i == 0
                    || i == last
                    || (band.dates[i].ends_with("-12-31")
                        && year_of(&band.dates[i]).is_some_and(|y| y % 5 == 0))
            })
            .collect();
        let _ = writeln!(
            out,
            "\nBalances on {path} (nominal, every fifth year end plus the first and last points):"
        );
        let header: Vec<String> = results
            .account_series
            .iter()
            .map(|s| account_name(Some(s.account_id)))
            .collect();
        let _ = writeln!(out, "date,{}", header.join(","));
        for i in rows {
            let values: Vec<String> = results
                .account_series
                .iter()
                .map(|s| {
                    s.values
                        .get(i)
                        .map(|v| format!("{v:.0}"))
                        .unwrap_or_default()
                })
                .collect();
            let _ = writeln!(out, "{},{}", band.dates[i], values.join(","));
        }
    }
    let _ = writeln!(out, "</run>");
}

fn render_existing(out: &mut String, rules: &[Draft]) {
    let _ = writeln!(out, "\n<existing_notes>");
    if rules.is_empty() {
        let _ = writeln!(out, "None.");
    }
    for d in rules {
        let _ = writeln!(
            out,
            "- [{} / {}] {}{}",
            serde_json::to_value(d.kind)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            serde_json::to_value(d.section)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            d.title,
            d.paths
                .iter()
                .map(|p| {
                    let changes: Vec<Change> = p.changes().cloned().collect();
                    format!(
                        "\n  path \"{}\"{}: {} (changes: {})",
                        p.key,
                        if p.recommended { ", recommended" } else { "" },
                        p.label,
                        edits(&changes).into_iter().collect::<Vec<_>>().join(", ")
                    )
                })
                .collect::<String>()
        );
    }
    let _ = writeln!(out, "</existing_notes>");
}
