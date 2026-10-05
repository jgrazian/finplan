//! Notes on the Portfolio: what is held, where, and how it is modelled.

use std::collections::{BTreeSet, HashMap};

use super::{Ctx, Draft, DraftPath, Evidence, Kind, Section, clip, list, money, rmd_age};
use crate::specs::EffectSpec;
use crate::suggest::Change;
use crate::templates::{
    ReinvestCashParams, RowRef, Template, expand_template_in, holdings, largest_taxable,
};

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

/// A stretch of consecutive year-ends at which an account's cash sat above a
/// multiple of that year's spending, and its highest point.
#[derive(Debug, Clone, Copy)]
struct CashRun {
    first: i64,
    last: i64,
    peak_year: i64,
    peak: f64,
}

impl CashRun {
    fn years(&self) -> usize {
        (self.last - self.first + 1) as usize
    }
}

/// The plan's first year-end that closes a whole calendar year.
fn opening_year(ctx: &Ctx) -> Option<i64> {
    ctx.results
        .cash_flows
        .iter()
        .map(|c| c.year)
        .find(|year| !ctx.is_partial_first_year(*year))
}

/// Every run of year-ends, in order, at which `account_id` held more cash
/// than `line` years of that year's spending on the shown path. The plan's
/// partial first year, and a year with no spending, break a run.
fn cash_runs(ctx: &Ctx, account_id: i64, line: f64) -> Vec<CashRun> {
    let mut runs: Vec<CashRun> = Vec::new();
    let mut open = false;
    for flow in &ctx.results.cash_flows {
        let year = flow.year;
        let cash = (!ctx.is_partial_first_year(year) && flow.expenses > 0.0)
            .then(|| ctx.year_end_cash(account_id, year))
            .flatten()
            .filter(|cash| *cash > line * flow.expenses);
        let Some(cash) = cash else {
            open = false;
            continue;
        };
        match runs.last_mut() {
            Some(run) if open => {
                run.last = year;
                if cash > run.peak {
                    (run.peak_year, run.peak) = (year, cash);
                }
            }
            _ => runs.push(CashRun {
                first: year,
                last: year,
                peak_year: year,
                peak: cash,
            }),
        }
        open = true;
    }
    runs
}

/// The run of idle year-ends that starts with the plan's first whole year:
/// cash held from the start, which is `idle_bank_cash`'s case.
fn idle_from_start(ctx: &Ctx, account_id: i64) -> Option<CashRun> {
    let opening = opening_year(ctx)?;
    cash_runs(ctx, account_id, IDLE_YEARS_OF_SPENDING)
        .into_iter()
        .find(|run| run.first == opening)
}

/// A bank account holding more than two years of spending from the plan's
/// start, year after year, on the shown path. Cash that builds up only later
/// is `cash_accumulates`'s.
pub(super) fn idle_bank_cash(ctx: &Ctx) -> Vec<Draft> {
    let mut drafts = Vec::new();
    let mut bank_ids: Vec<i64> = ctx.graph.bank.keys().copied().collect();
    bank_ids.sort_unstable();
    for account_id in bank_ids {
        let Some(CashRun {
            first,
            last,
            peak_year,
            peak,
        }) = idle_from_start(ctx, account_id)
        else {
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

/// How far above two years of spending cash must sit to count as building up.
/// The buffer `ReinvestCash` keeps is two years of spending in plan-start
/// dollars, grown with inflation to the day it runs, so a balance held at the
/// buffer sits a little above two years of the spending paid earlier in the
/// year: that is the buffer, not a build-up.
const ACCUMULATE_MARGIN: f64 = 1.1;
/// Consecutive year-ends above that line before `cash_accumulates` fires.
const ACCUMULATE_MIN_RUN: usize = 3;

/// Cash, in a bank or uninvested in an investment account, that rises above
/// two years of spending after the plan begins and stays there for three or
/// more year-ends on the shown path: typically from the first RMD year, when
/// distributions exceed spending. Cash idle from the plan's first year-end is
/// `idle_bank_cash`'s; a build-up counts only once that stretch has ended, so
/// the two never describe the same years.
pub(super) fn cash_accumulates(ctx: &Ctx) -> Vec<Draft> {
    let Some(opening) = opening_year(ctx) else {
        return Vec::new();
    };
    let mut accounts: Vec<_> = ctx
        .graph
        .accounts
        .iter()
        .filter(|a| ctx.graph.bank.contains_key(&a.id) || ctx.graph.investment.contains_key(&a.id))
        .collect();
    accounts.sort_by_key(|a| (a.sort_order, a.id));
    let mut drafts = Vec::new();
    for account in accounts {
        let account_id = account.id;
        let after = idle_from_start(ctx, account_id).map_or(opening, |run| run.last);
        let Some(run) = cash_runs(ctx, account_id, IDLE_YEARS_OF_SPENDING * ACCUMULATE_MARGIN)
            .into_iter()
            .find(|run| run.first > after && run.years() >= ACCUMULATE_MIN_RUN)
        else {
            continue;
        };
        drafts.push(accumulation_note(ctx, account_id, run));
    }
    drafts
}

fn accumulation_note(ctx: &Ctx, account_id: i64, run: CashRun) -> Draft {
    let CashRun {
        first,
        last,
        peak_year,
        peak,
    } = run;
    let name = ctx.account_name(account_id);
    let bank = ctx.graph.bank.contains_key(&account_id);
    let cash_first = ctx.year_end_cash(account_id, first).unwrap_or(0.0);
    let spent_first = ctx.expenses(first).unwrap_or(0.0);
    let spent_peak = ctx.expenses(peak_year).unwrap_or(0.0);

    // RMDs paid into the account, when they start by the build-up's first year.
    let rmd_year = ctx.birth_year().map(|b| b + i64::from(rmd_age(b)));
    let rmds_in = ctx.events.iter().flat_map(|e| &e.effects).any(|effect| {
        let mut found = false;
        visit(effect, &mut |e| {
            found |= matches!(e, EffectSpec::ApplyRmd { to_account_id, .. } if *to_account_id == account_id)
        });
        found
    });
    let cause = match rmd_year {
        Some(year) if rmds_in && year <= first => {
            format!(" From {year}, the RMDs paid into {name} exceed what the plan spends.")
        }
        _ => String::new(),
    };

    // The buffer: two years of the lowest spending from the build-up on, in
    // plan-start dollars, so the cash kept stays under the line every year.
    let spending = ctx
        .results
        .cash_flows
        .iter()
        .filter(|c| c.year >= first && c.expenses > 0.0)
        .map(|c| c.expenses / ctx.inflation(c.year))
        .fold(f64::INFINITY, f64::min);
    let path = reinvest_path(ctx, account_id, spending.is_finite().then_some(spending));
    let fix = match &path {
        Ok((to, _)) => {
            let into = if *to == account_id {
                format!("{name}'s own holdings")
            } else {
                format!("{}'s holdings", ctx.account_name(*to))
            };
            format!(
                " A yearly December 30 event keeps two years of spending as cash, {} in {} \
                 dollars grown with inflation, and invests the rest in {into}.",
                money((2.0 * spending).round()),
                ctx.start().map_or(first, |s| s.year),
            )
        }
        Err(why) => format!(" {why}"),
    };
    let paths = match path {
        Ok((to, changes)) => {
            let label = if to == account_id {
                format!("Invest {name}'s cash above two years of spending")
            } else {
                format!(
                    "Reinvest cash above two years of spending in {}",
                    ctx.account_name(to)
                )
            };
            vec![DraftPath::only(clip(&label), changes)]
        }
        Err(_) => Vec::new(),
    };

    let mut evidence = vec![
        Evidence::Ledger {
            year: first,
            event_id: None,
            account_id: Some(account_id),
        },
        Evidence::Stat {
            name: "first year of the build-up".into(),
            value: first as f64,
        },
        Evidence::Stat {
            name: format!("peak cash {peak_year}"),
            value: peak,
        },
        Evidence::Stat {
            name: format!("expenses {peak_year}"),
            value: spent_peak,
        },
    ];
    if bank {
        evidence.push(Evidence::AccountSeries {
            account_id,
            date: format!("{peak_year:04}-12-31"),
            value: peak,
        });
    }
    let (title, summary, held) = if bank {
        (
            format!(
                "{name} builds up cash from {first}, peaking at {} in {peak_year}",
                money(peak)
            ),
            format!(
                "From {first} more money reaches {name} than the plan spends, and the surplus \
                 sits as cash instead of being invested."
            ),
            "in cash",
        )
    } else {
        (
            format!(
                "Cash builds up uninvested in {name} from {first}, peaking at {} in {peak_year}",
                money(peak)
            ),
            format!(
                "From {first} cash reaches {name} faster than withdrawals spend it, and sits \
                 there uninvested."
            ),
            "of uninvested cash",
        )
    };
    let through = if Some(last) == ctx.results.cash_flows.last().map(|c| c.year) {
        "to the plan's end".to_string()
    } else {
        format!("through {last}")
    };
    Draft {
        rule: "cash_accumulates",
        kind: Kind::Fix,
        section: Section::Portfolio,
        title,
        summary,
        reasoning: format!(
            "On the median path {name} ends {first} with {} {held}, {:.1} years of that year's \
             {} spending. It stays above two years of spending for {} year-ends, {through}, and \
             peaks at {} in {peak_year}.{cause}{fix}",
            money(cash_first),
            if spent_first > 0.0 {
                cash_first / spent_first
            } else {
                0.0
            },
            money(spent_first),
            run.years(),
            money(peak),
        ),
        evidence,
        paths,
    }
}

/// The `ReinvestCash` template for the cash in `account_id`, keeping two
/// years of `spending` (plan-start dollars) or none: in place for an
/// investment account, into the plan's largest taxable account for a bank.
/// The account it invests in and the changes, or why there is no path.
pub(super) fn reinvest_path(
    ctx: &Ctx,
    account_id: i64,
    spending: Option<f64>,
) -> Result<(i64, Vec<Change>), String> {
    let name = ctx.account_name(account_id);
    let to = if ctx.graph.bank.contains_key(&account_id) {
        largest_taxable(ctx.graph).ok_or_else(|| {
            "The plan has no taxable account with holdings to reinvest the cash in.".to_string()
        })?
    } else {
        account_id
    };
    if holdings(ctx.graph, to).is_empty() {
        return Err(format!("{name} holds no investments to buy with the cash."));
    }
    let template = Template::ReinvestCash(ReinvestCashParams {
        name: None,
        from_account_id: RowRef::Id(account_id),
        to_account_id: Some(RowRef::Id(to)),
        buffer_years: Some(if spending.is_some() { 2.0 } else { 0.0 }),
        annual_spending: spending.map(f64::round),
    });
    expand_template_in(ctx.graph, "", &template)
        .map(|expansion| (to, expansion.changes))
        .map_err(|e| format!("Reinvesting the cash does not fit this plan: {e}."))
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
