//! Notes on the Portfolio: what is held, where, and how it is modelled.

use std::collections::{BTreeSet, HashMap};

use super::{Ctx, Draft, Evidence, Kind, Section, list, money};
use crate::specs::EffectSpec;

/// A lot counts as bought at today's price when its basis is within this
/// fraction of units × initial price.
const BASIS_TOLERANCE: f64 = 0.01;
/// Share of taxable lot value that must look that way before the plan as a
/// whole is said to assume no gains.
const BASIS_SHARE: f64 = 0.8;

/// Taxable lots all bought on the start date at today's price: the plan
/// assumes no embedded gains, which only the user can confirm.
pub(super) fn cost_basis_equals_value(ctx: &Ctx) -> Vec<Draft> {
    let graph = ctx.graph;
    let start = graph.scenario.start_date.as_str();
    let prices: HashMap<i64, (f64, &str)> = graph
        .assets
        .iter()
        .map(|a| (a.id, (a.initial_price, a.name.as_str())))
        .collect();

    let mut total = 0.0;
    let mut at_cost = 0.0;
    let mut lots = 0;
    let mut lots_at_cost = 0;
    // The largest lot bought at today's price, to show the arithmetic on.
    let mut example: Option<(f64, &str, f64, f64)> = None;
    for (account_id, investment) in &graph.investment {
        if investment.tax_status != "Taxable" {
            continue;
        }
        for lot in graph.positions.get(account_id).into_iter().flatten() {
            let Some(&(price, name)) = prices.get(&lot.asset_id) else {
                continue;
            };
            let value = lot.units * price;
            if value <= 0.0 {
                continue;
            }
            total += value;
            lots += 1;
            if lot.purchase_date == start
                && (lot.cost_basis - value).abs() <= BASIS_TOLERANCE * value
            {
                at_cost += value;
                lots_at_cost += 1;
                if example.is_none_or(|(v, ..)| value > v) {
                    example = Some((value, name, lot.units, price));
                }
            }
        }
    }
    if lots < 2 || at_cost < BASIS_SHARE * total {
        return Vec::new();
    }
    let Some((basis, name, units, price)) = example else {
        return Vec::new();
    };

    let title = if lots_at_cost == lots {
        format!("Every taxable lot cost exactly what it is worth today ({lots} lots)")
    } else {
        format!("{lots_at_cost} of {lots} taxable lots cost exactly what they are worth today")
    };
    let reasoning = format!(
        "Those lots are dated {start}, the plan's start, with a cost basis equal to today's \
         value: {name} is {units} × {price} = {basis}. The plan therefore assumes {held} of \
         taxable holdings carry no gains, and taxes only growth after {start}. If they were \
         bought earlier for less, every sale — a down payment, a sweep, retirement \
         withdrawals — is taxed too lightly.",
        units = trim_units(units),
        price = money(price),
        basis = money(basis),
        held = money(at_cost),
    );
    vec![Draft {
        rule: "cost_basis_equals_value",
        kind: Kind::Check,
        section: Section::Portfolio,
        title,
        summary: format!(
            "The plan treats {} of taxable holdings as having no gains, so if they were bought \
             for less, every sale is taxed too lightly.",
            money(at_cost)
        ),
        reasoning,
        evidence: vec![
            Evidence::Stat {
                name: "taxable lot value at cost".into(),
                value: at_cost,
            },
            Evidence::Stat {
                name: "taxable lot value".into(),
                value: total,
            },
        ],
        paths: Vec::new(),
    }]
}

fn trim_units(units: f64) -> String {
    let text = format!("{units:.1}");
    text.trim_end_matches(".0").to_string()
}

/// How many years of that year's spending a bank balance may hold before it
/// counts as idle.
const IDLE_YEARS_OF_SPENDING: f64 = 2.0;
/// Consecutive year-ends above that line before the note fires.
const IDLE_MIN_RUN: usize = 5;

/// A bank account holding more than two years of spending, year after year,
/// on the shown path.
pub(super) fn idle_bank_cash(ctx: &Ctx) -> Vec<Draft> {
    let mut drafts = Vec::new();
    let mut bank_ids: Vec<i64> = ctx.graph.bank.keys().copied().collect();
    bank_ids.sort_unstable();
    for account_id in bank_ids {
        // The longest run of years above the line: (first, last, peak year, peak).
        let mut best: Option<(i64, i64, i64, f64)> = None;
        let mut current: Option<(i64, i64, i64, f64)> = None;
        for flow in &ctx.results.cash_flows {
            let year = flow.year;
            let idle = !ctx.is_partial_first_year(year)
                && flow.expenses > 0.0
                && ctx
                    .year_end(account_id, year)
                    .is_some_and(|b| b > IDLE_YEARS_OF_SPENDING * flow.expenses);
            if !idle {
                current = None;
                continue;
            }
            let balance = ctx.year_end(account_id, year).unwrap_or(0.0);
            current = Some(match current {
                Some((first, _, peak_year, peak)) if balance <= peak => {
                    (first, year, peak_year, peak)
                }
                Some((first, ..)) => (first, year, year, balance),
                None => (year, year, year, balance),
            });
            if let Some(run) = current
                && best.is_none_or(|b| run.1 - run.0 > b.1 - b.0)
            {
                best = Some(run);
            }
        }
        let Some((first, last, peak_year, peak)) = best else {
            continue;
        };
        let years = (last - first + 1) as usize;
        if years < IDLE_MIN_RUN {
            continue;
        }
        let name = ctx.account_name(account_id);
        let spending = ctx.expenses(peak_year).unwrap_or(0.0);
        let rate = ctx
            .graph
            .bank
            .get(&account_id)
            .and_then(|b| ctx.graph.return_profiles.get(&b.return_profile_id))
            .map_or_else(
                || "its cash".to_string(),
                |p| format!("the {} rate", p.name),
            );
        let mut investment: Vec<String> = ctx
            .graph
            .investment
            .keys()
            .map(|id| ctx.account_name(*id))
            .collect();
        investment.sort();
        let elsewhere = match investment.len() {
            0 => String::new(),
            1 | 2 => format!(
                " Beyond a buffer, that money could be invested in {} instead.",
                list(&investment)
            ),
            _ => " Beyond a buffer, that money could be invested instead.".to_string(),
        };
        drafts.push(Draft {
            rule: "idle_bank_cash",
            kind: Kind::Check,
            section: Section::Portfolio,
            title: format!(
                "{name} holds over two years of spending for {years} years running, peaking at {} in {peak_year}",
                money(peak)
            ),
            summary: format!(
                "{name} keeps far more cash than the plan spends, earning {rate} rather than \
                 investment returns."
            ),
            reasoning: format!(
                "On the median path {name} ends every year from {first} through {last} with \
                 more than twice that year's spending, and peaks at {} in {peak_year} — {:.1} \
                 years of that year's {} spending — earning {rate}.{elsewhere}",
                money(peak),
                if spending > 0.0 { peak / spending } else { 0.0 },
                money(spending),
            ),
            evidence: vec![
                Evidence::AccountSeries {
                    account_id,
                    date: format!("{peak_year:04}-12-31"),
                    value: peak,
                },
                Evidence::Stat {
                    name: format!("expenses {peak_year}"),
                    value: spending,
                },
            ],
            paths: Vec::new(),
        });
    }
    drafts
}

/// Accounts with a contribution limit that no event ever pays into, in a plan
/// that has income to contribute.
pub(super) fn unused_contribution_limits(ctx: &Ctx) -> Vec<Draft> {
    let mut funded = BTreeSet::new();
    let mut income_events = Vec::new();
    let mut income_accounts = BTreeSet::new();
    for event in &ctx.events {
        for effect in &event.effects {
            visit(effect, &mut |e| match e {
                EffectSpec::Income { to_account_id, .. } => {
                    funded.insert(*to_account_id);
                    income_accounts.insert(*to_account_id);
                    if !income_events.contains(&event.name) {
                        income_events.push(event.name.clone());
                    }
                }
                EffectSpec::CashTransfer { to_account_id, .. }
                | EffectSpec::AssetPurchase { to_account_id, .. }
                | EffectSpec::Sweep { to_account_id, .. }
                | EffectSpec::RsuVesting { to_account_id, .. }
                | EffectSpec::ApplyRmd { to_account_id, .. } => {
                    funded.insert(*to_account_id);
                }
                _ => {}
            });
        }
    }
    let income_years: Vec<_> = ctx
        .results
        .cash_flows
        .iter()
        .filter(|c| c.income > 0.0)
        .collect();
    if income_events.is_empty() || income_years.is_empty() {
        return Vec::new();
    }

    let mut unused: Vec<(i64, f64, Option<&str>)> = ctx
        .graph
        .investment
        .values()
        .filter_map(|inv| {
            let limit = inv.contribution_limit.filter(|l| *l > 0.0)?;
            (!funded.contains(&inv.account_id)).then_some((
                inv.account_id,
                limit,
                inv.contribution_period.as_deref(),
            ))
        })
        .collect();
    if unused.is_empty() {
        return Vec::new();
    }
    unused.sort_by_key(|(id, ..)| *id);

    let names: Vec<String> = unused
        .iter()
        .map(|(id, ..)| ctx.account_name(*id))
        .collect();
    let limits: Vec<String> = unused
        .iter()
        .map(|(_, limit, period)| match *period {
            Some("Monthly") => format!("{} a month", money(*limit)),
            _ => format!("{} a year", money(*limit)),
        })
        .collect();
    let contributed: f64 = income_years.iter().map(|c| c.contributions).sum();
    let landing: Vec<String> = income_accounts
        .iter()
        .map(|id| ctx.account_name(*id))
        .collect();
    let column = if contributed.abs() < 0.5 {
        format!(
            "the contributions column is $0 in all {} years with income",
            income_years.len()
        )
    } else {
        format!(
            "contributions total {} across the {} years with income",
            money(contributed),
            income_years.len()
        )
    };
    let each = if unused.len() == 1 {
        "It has"
    } else {
        "Each has"
    };
    vec![Draft {
        rule: "unused_contribution_limits",
        kind: Kind::Check,
        section: Section::Portfolio,
        title: format!("Nothing is ever contributed to {}", list(&names)),
        summary: format!(
            "{} {} a contribution limit but no event pays into {}, so that tax-advantaged room \
             goes unused.",
            list(&names),
            if names.len() == 1 { "has" } else { "have" },
            if names.len() == 1 { "it" } else { "them" },
        ),
        reasoning: format!(
            "{each} a contribution limit set ({}) but no event pays into it. {} lands in {} \
             instead, and {column}.",
            list(&limits),
            list(&income_events),
            list(&landing),
        ),
        evidence: vec![
            Evidence::Stat {
                name: "contributions".into(),
                value: contributed,
            },
            Evidence::Stat {
                name: "years with income".into(),
                value: income_years.len() as f64,
            },
        ],
        paths: Vec::new(),
    }]
}

/// Walk an effect and the branches of any `Random` inside it.
pub(super) fn visit<'e>(effect: &'e EffectSpec, f: &mut impl FnMut(&'e EffectSpec)) {
    f(effect);
    if let EffectSpec::Random {
        on_true, on_false, ..
    } = effect
    {
        visit(on_true, f);
        if let Some(on_false) = on_false {
            visit(on_false, f);
        }
    }
}

/// Words in an asset's description that name a company rather than a fund.
const COMPANY_MARKERS: &[&str] = &[
    " inc",
    " corp",
    " class ",
    " ltd",
    " plc",
    " holdings",
    " co.",
];
/// Words that name a pooled fund; any of them rules out a single stock.
const FUND_MARKERS: &[&str] = &["fund", "etf", "index", "trust", "portfolio", "admiral"];

/// A description keyword, and what the mapped profile must mention to match it.
struct Mismatch {
    keyword: &'static str,
    /// Accepted when the profile's name, description or class contains any.
    profile_mentions: &'static [&'static str],
    why: &'static str,
}

const MISMATCHES: &[Mismatch] = &[
    Mismatch {
        keyword: "emerging",
        profile_mentions: &["emerging"],
        why: "an emerging-markets fund, but its profile describes other markets",
    },
    Mismatch {
        keyword: "target retirement",
        profile_mentions: &["target", "balanced", "blend"],
        why: "a target-date fund, which holds bonds and shifts toward them as the date nears; \
              a single equity profile overstates its risk and return late in the plan",
    },
    Mismatch {
        keyword: "bond",
        profile_mentions: &["bond"],
        why: "a bond fund modelled with a profile that is not bonds",
    },
];

/// Held assets whose return model doesn't fit them: unmapped holdings, a
/// single stock moving in lockstep with an index, a fund on the wrong profile.
///
/// An unmapped asset held only through a property account is left to
/// preflight, which already flags every unmapped asset; this rule adds a note
/// only where it can say how much money is affected.
pub(super) fn unmapped_or_mismatched_assets(ctx: &Ctx) -> Vec<Draft> {
    let graph = ctx.graph;
    let mut held: HashMap<i64, f64> = HashMap::new();
    for lots in graph.positions.values() {
        for lot in lots {
            if let Some(asset) = graph.assets.iter().find(|a| a.id == lot.asset_id) {
                *held.entry(asset.id).or_default() += lot.units * asset.initial_price;
            }
        }
    }

    let mut drafts = Vec::new();
    let mut assets: Vec<_> = graph.assets.iter().collect();
    assets.sort_by_key(|a| (a.sort_order, a.id));
    for asset in assets {
        let value = held.get(&asset.id).copied().unwrap_or(0.0);
        if value <= 0.0 {
            continue;
        }
        let evidence = vec![Evidence::Stat {
            name: format!("{} held", asset.name),
            value,
        }];
        let Some(profile_id) = asset.return_profile_id else {
            drafts.push(Draft {
                rule: "unmapped_or_mismatched_assets",
                kind: Kind::Check,
                section: Section::Portfolio,
                title: format!(
                    "{} has no return assumption, so its {} stays flat",
                    asset.name,
                    money(value)
                ),
                summary: format!(
                    "With no return profile, {} neither grows nor swings, and loses value to \
                     inflation every year.",
                    asset.name
                ),
                reasoning: format!(
                    "{} is held in the portfolio but mapped to no return profile, so the \
                     simulation keeps its price fixed in nominal dollars for the whole plan: \
                     no growth, and a real loss to inflation every year.",
                    asset.name
                ),
                evidence,
                paths: Vec::new(),
            });
            continue;
        };
        let Some(profile) = graph.return_profiles.get(&profile_id) else {
            continue;
        };
        let description =
            format!(" {} ", asset.description.as_deref().unwrap_or("")).to_lowercase();
        let profile_text = format!(
            "{} {} {}",
            profile.name,
            profile.description.as_deref().unwrap_or(""),
            profile.asset_class.as_deref().unwrap_or("")
        )
        .to_lowercase();

        let single_stock = COMPANY_MARKERS.iter().any(|m| description.contains(m))
            && !FUND_MARKERS.iter().any(|m| description.contains(m));
        if single_stock && asset.tracking_error.is_none() {
            drafts.push(Draft {
                rule: "unmapped_or_mismatched_assets",
                kind: Kind::Check,
                section: Section::Portfolio,
                title: format!("{} moves exactly like {}", asset.name, profile.name),
                summary: format!(
                    "A single stock is modelled on an index with no tracking error, so the plan \
                     understates how much its {} can swing.",
                    money(value)
                ),
                reasoning: format!(
                    "{} ({}) is one company, but it is modelled on {} with no tracking error, \
                     so every year it earns exactly what the index earns. {} is held. A single \
                     stock swings well beyond its index; a tracking error models that spread.",
                    asset.name,
                    asset.description.as_deref().unwrap_or("").trim(),
                    profile.name,
                    money(value),
                ),
                evidence,
                paths: Vec::new(),
            });
            continue;
        }
        if let Some(mismatch) = MISMATCHES.iter().find(|m| {
            description.contains(m.keyword)
                && !m.profile_mentions.iter().any(|p| profile_text.contains(p))
        }) {
            drafts.push(Draft {
                rule: "unmapped_or_mismatched_assets",
                kind: Kind::Check,
                section: Section::Portfolio,
                title: format!("{} is modelled as {}", asset.name, profile.name),
                summary: format!(
                    "{}'s description doesn't match the {} return profile, so its {} may grow \
                     at the wrong rate.",
                    asset.name,
                    profile.name,
                    money(value)
                ),
                reasoning: format!(
                    "{} ({}) is {}. {} is held.",
                    asset.name,
                    asset.description.as_deref().unwrap_or("").trim(),
                    mismatch.why,
                    money(value),
                ),
                evidence,
                paths: Vec::new(),
            });
        }
    }
    drafts
}
