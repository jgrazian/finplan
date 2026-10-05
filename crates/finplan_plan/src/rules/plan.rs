//! Notes on events: effects that do something other than what they appear to.

use serde_json::json;

use finplan_core::evaluate::bracket_ceiling;
use finplan_core::model::{RmdTable, TaxBracket};
use finplan_core::taxes::{indexed_brackets, marginal_rate};

use super::portfolio::{reinvest_path, visit};
use super::{
    Ctx, Draft, DraftPath, DraftStep, Evidence, Kind, Section, clip, fixed_amount, list, money,
    percent, rmd_age, year_of,
};
use crate::results::view::CashFlow;
use crate::specs::{AmountSpec, EffectSpec, Interval, TriggerSpec};
use crate::suggest::{Change, ChangeOp, ChangeTarget};
use crate::templates::{
    RothConversionsParams, RowRef, Template, When, expand_template_in, investment_account,
    largest_taxable,
};

/// A one-off event that sweeps investments into an account and then pays an
/// expense from it, while that account already holds the expense: the sale
/// only realises gains and taxes early.
pub(super) fn sweep_sells_while_cash(ctx: &Ctx) -> Vec<Draft> {
    let mut drafts = Vec::new();
    for event in &ctx.events {
        let once = event.fires_once
            || matches!(
                event.trigger,
                TriggerSpec::Age { .. }
                    | TriggerSpec::Date { .. }
                    | TriggerSpec::RelativeToEvent { .. }
            );
        if !once {
            continue;
        }
        let Some(fires) = ctx.fires(&event.trigger) else {
            continue;
        };
        let inflation = ctx.inflation(fires.year);
        for (i, effect) in event.effects.iter().enumerate() {
            let EffectSpec::Sweep {
                to_account_id,
                amount,
                ..
            } = effect
            else {
                continue;
            };
            let account = *to_account_id;
            let Some((swept, _)) = fixed_amount(amount, inflation) else {
                continue;
            };
            // The expense the sweep funds: the first later one from the same account.
            let Some(expense) = event.effects[i + 1..].iter().find_map(|e| match e {
                EffectSpec::Expense {
                    from_account_id,
                    amount,
                } if *from_account_id == account => {
                    Some(fixed_amount(amount, inflation).map_or(swept, |(v, _)| v))
                }
                _ => None,
            }) else {
                continue;
            };
            let Some((date, held)) = ctx.balance_before(account, &fires.date()) else {
                continue;
            };
            if held < expense {
                continue;
            }
            let Ok(expect) = serde_json::to_value(effect) else {
                continue;
            };
            let name = ctx.account_name(account);
            let withdrawals = ctx
                .results
                .cash_flows
                .iter()
                .find(|c| c.year == fires.year)
                .map(|c| c.withdrawals);
            let year_withdrawals = withdrawals
                .filter(|w| *w > 0.0)
                .map(|w| format!(" Withdrawals that year come to {}.", money(w)))
                .unwrap_or_default();
            let mut evidence = vec![
                Evidence::Ledger {
                    year: fires.year,
                    event_id: Some(event.id),
                    account_id: None,
                },
                Evidence::AccountSeries {
                    account_id: account,
                    date: date.to_string(),
                    value: held,
                },
            ];
            if let Some(w) = withdrawals {
                evidence.push(Evidence::Stat {
                    name: format!("withdrawals {}", fires.year),
                    value: w,
                });
            }
            drafts.push(Draft {
                rule: "sweep_sells_while_cash",
                kind: Kind::Fix,
                section: Section::Plan,
                title: format!(
                    "{} sells {} of investments while {name} holds {}",
                    event.name,
                    money(swept),
                    money(held)
                ),
                summary: format!(
                    "{name} already has the cash, so selling investments first only brings \
                     gains and their taxes forward."
                ),
                reasoning: format!(
                    "In {year} {event} first sweeps {swept} into {name} by selling investments, \
                     then pays {expense} from {name}. On the median path {name} already holds \
                     {held} at {when}, so the sale only brings gains and their taxes forward.\
                     {year_withdrawals} Removing the sweep pays the {expense} from cash.",
                    when = when(date),
                    year = fires.year,
                    event = event.name,
                    swept = money(swept),
                    expense = money(expense),
                    held = money(held),
                ),
                evidence,
                paths: vec![DraftPath::only(
                    clip(&format!("Remove the sweep; pay from {name}'s cash")),
                    vec![Change {
                        op: ChangeOp::Remove,
                        target: ChangeTarget::Event(event.id),
                        path: format!("/effects/{i}"),
                        expect: Some(expect),
                        value: None,
                    }],
                )],
            });
        }
    }
    drafts
}

/// A payment into a loan that grows with inflation. Loan payments are fixed in
/// dollars; an inflation-adjusted one pays the loan off early and loads more
/// of its cost into later years.
pub(super) fn liability_payment_inflation_adjusted(ctx: &Ctx) -> Vec<Draft> {
    let mut drafts = Vec::new();
    for event in &ctx.events {
        for (i, effect) in event.effects.iter().enumerate() {
            let EffectSpec::CashTransfer {
                to_account_id,
                amount,
                ..
            } = effect
            else {
                continue;
            };
            if ctx.flavor(*to_account_id) != Some("Liability") {
                continue;
            }
            let AmountSpec::InflationAdjusted { inner } = amount else {
                continue;
            };
            let AmountSpec::Fixed { value } = inner.as_ref() else {
                continue;
            };
            let (Ok(expect), Ok(fixed)) =
                (serde_json::to_value(amount), serde_json::to_value(inner))
            else {
                continue;
            };
            let loan = ctx.account_name(*to_account_id);
            let per = match &event.trigger {
                TriggerSpec::Repeating { interval, .. } => per(*interval),
                _ => "",
            };
            let first_year = ctx
                .fires(&event.trigger)
                .map(|m| m.year)
                .or_else(|| ctx.start().map(|m| m.year));
            let payoff = payoff(ctx, *to_account_id);
            let amortizing = match &event.trigger {
                TriggerSpec::Repeating {
                    interval: Interval::Monthly,
                    ..
                } => amortizing_payment(ctx, *to_account_id),
                _ => None,
            }
            // Only an alternative when it is a different figure.
            .filter(|a| (a.payment - value).abs() > 0.02 * value.abs());

            let mut evidence = Vec::new();
            let growth = match (first_year, &payoff) {
                (Some(first), Some((date, _))) => {
                    let year = year_of(date);
                    let start = value * ctx.inflation(first);
                    let end = value * ctx.inflation(year);
                    evidence.push(Evidence::AccountSeries {
                        account_id: *to_account_id,
                        date: date.clone(),
                        value: 0.0,
                    });
                    evidence.push(Evidence::Stat {
                        name: format!("payment {year}"),
                        value: end,
                    });
                    Some((first, start, year, end))
                }
                _ => None,
            };
            let title = match growth {
                Some((_, start, _, end)) => format!(
                    "{} rises with inflation, from {}{per} to {}{per}",
                    event.name,
                    money(start),
                    money(end)
                ),
                None => format!("{}'s payment rises with inflation", event.name),
            };
            let path_note = match growth {
                Some((first, start, year, end)) => format!(
                    " On the median path it is {}{per} in {first} and {}{per} by {year}, when \
                     {loan} reaches zero.",
                    money(start),
                    money(end)
                ),
                None => String::new(),
            };
            drafts.push(Draft {
                rule: "liability_payment_inflation_adjusted",
                kind: Kind::Fix,
                section: Section::Plan,
                title,
                summary: format!(
                    "A loan payment is fixed in dollars, but this one grows with inflation, so \
                     {loan} is paid off early and its cost lands in later years."
                ),
                reasoning: format!(
                    "A loan payment is fixed in dollars, but this one is inflation-adjusted, so \
                     it grows every year.{path_note} That pays {loan} off sooner than its \
                     schedule and puts more of its cost into later years. A fixed {}{per} \
                     matches how the loan is paid; check it against the loan's amortising \
                     payment.",
                    money(*value)
                ),
                evidence,
                paths: {
                    let replace = |value: serde_json::Value| Change {
                        op: ChangeOp::Replace,
                        target: ChangeTarget::Event(event.id),
                        path: format!("/effects/{i}/amount"),
                        expect: Some(expect.clone()),
                        value: Some(value),
                    };
                    let mut paths = vec![DraftPath::single(
                        "fixed",
                        clip(&format!("Fix the payment at {}{per}", money(*value))),
                        true,
                        vec![replace(fixed)],
                    )];
                    if let Some(a) = amortizing {
                        let mut path = DraftPath::single(
                            "amortizing",
                            clip(&format!(
                                "Fix it at the {}-year payment, {}{per}",
                                a.months / 12,
                                money(a.payment)
                            )),
                            false,
                            vec![replace(serde_json::json!({
                                "kind": "Fixed",
                                "value": (a.payment * 100.0).round() / 100.0,
                            }))],
                        );
                        path.reasoning = Some(format!(
                            "The payment that clears {} of {loan} over {} years at {:.2}%{}.",
                            money(a.principal),
                            a.months / 12,
                            a.rate * 100.0,
                            if a.assumed_term {
                                ", assuming a 30-year term since the plan does not give one"
                            } else {
                                ""
                            }
                        ));
                        paths.push(path);
                    }
                    paths
                },
            });
        }
    }
    drafts
}

/// A loan's level monthly payment, from what the plan says about it.
struct Amortizing {
    principal: f64,
    rate: f64,
    months: i64,
    /// The plan gives no term; 30 years is assumed.
    assumed_term: bool,
    payment: f64,
}

/// The level payment that clears the loan: its principal (at the start, or
/// what an event draws into it, in that year's dollars on the shown path) at
/// its rate over its term. None when the plan does not say enough.
fn amortizing_payment(ctx: &Ctx, account_id: i64) -> Option<Amortizing> {
    let row = ctx.graph.liability.get(&account_id)?;
    // Also refuses NaN.
    if row.interest_rate.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return None;
    }
    let principal = if row.principal.abs() > 0.5 {
        row.principal.abs()
    } else {
        ctx.events.iter().find_map(|event| {
            let year = ctx.fires(&event.trigger)?.year;
            event.effects.iter().find_map(|effect| match effect {
                EffectSpec::AdjustBalance {
                    account_id: id,
                    amount,
                } if *id == account_id => {
                    fixed_amount(amount, ctx.inflation(year)).map(|(v, _)| v.abs())
                }
                _ => None,
            })
        })?
    };
    let assumed_term = row.term_months.is_none_or(|m| m <= 0);
    let months = row.term_months.filter(|m| *m > 0).unwrap_or(360);
    let r = row.interest_rate / 12.0;
    let payment = principal * r / (1.0 - (1.0 + r).powi(-(months as i32)));
    payment.is_finite().then_some(Amortizing {
        principal,
        rate: row.interest_rate,
        months,
        assumed_term,
        payment,
    })
}

/// `the end of 2030` for a year-end checkpoint, the date itself otherwise.
fn when(date: &str) -> String {
    match date.strip_suffix("-12-31") {
        Some(year) => format!("the end of {year}"),
        None => date.to_string(),
    }
}

fn per(interval: Interval) -> &'static str {
    match interval {
        Interval::Weekly => "/wk",
        Interval::BiWeekly => " every two weeks",
        Interval::Monthly => "/mo",
        Interval::Quarterly => "/qtr",
        Interval::Yearly => "/yr",
        Interval::Never => "",
    }
}

/// The first date the loan's balance returns to zero after being drawn, on
/// the shown path.
fn payoff(ctx: &Ctx, account_id: i64) -> Option<(String, f64)> {
    let balances = ctx.balances(account_id);
    let drawn = balances.iter().position(|(_, v)| *v < -0.5)?;
    balances[drawn..]
        .iter()
        .find(|(_, v)| *v >= -0.5)
        .map(|(d, v)| (d.to_string(), *v))
}

/// The plan's tax-deferred accounts that hold money when RMDs start in
/// `first`: the last balance before that year on the shown path (the first
/// one when RMDs are already due), or the opening value of an account the
/// run has no series for.
fn deferred_at(ctx: &Ctx, first: i64) -> Vec<(i64, String, f64)> {
    let jan1 = format!("{first:04}-01-01");
    let mut accounts: Vec<_> = ctx
        .graph
        .accounts
        .iter()
        .filter(|a| {
            ctx.graph
                .investment
                .get(&a.id)
                .is_some_and(|i| i.tax_status == "TaxDeferred")
        })
        .collect();
    accounts.sort_by_key(|a| (a.sort_order, a.id));
    accounts
        .into_iter()
        .filter_map(|a| {
            let balances = ctx.balances(a.id);
            let (date, value) = if balances.is_empty() {
                (
                    ctx.graph.scenario.start_date.clone(),
                    ctx.opening_value(a.id),
                )
            } else {
                let (date, value) = balances
                    .iter()
                    .take_while(|(d, _)| *d < jan1.as_str())
                    .last()
                    .or(balances.first())
                    .copied()?;
                (date.to_string(), value)
            };
            (value > 0.5).then_some((a.id, date, value))
        })
        .collect()
}

/// Tax-deferred money still held at the RMD age, inside the plan, with no
/// event that applies RMDs: the engine takes them only when an `ApplyRmd`
/// effect runs, so the run never withdraws or taxes them. A correctness note.
pub(super) fn rmd_missing(ctx: &Ctx) -> Vec<Draft> {
    let (Some(birth), Some(start), Some(end)) = (ctx.birth_year(), ctx.start(), ctx.end_year())
    else {
        return Vec::new();
    };
    let age = rmd_age(birth);
    let first = birth + i64::from(age);
    if first > end {
        return Vec::new();
    }
    let applies_rmd = ctx.events.iter().flat_map(|e| &e.effects).any(|effect| {
        let mut found = false;
        visit(effect, &mut |e| {
            found |= matches!(e, EffectSpec::ApplyRmd { .. })
        });
        found
    });
    if applies_rmd {
        return Vec::new();
    }
    let held = deferred_at(ctx, first);
    if held.is_empty() {
        return Vec::new();
    }
    let names = list(
        &held
            .iter()
            .map(|(id, ..)| ctx.account_name(*id))
            .collect::<Vec<_>>(),
    );
    let total: f64 = held.iter().map(|(.., v)| v).sum();
    // Already past the age at the plan's start: due from the start.
    let past = first <= start.year;
    let (when, due) = if past {
        (
            "at the plan's start".to_string(),
            format!("RMDs are due from the start: they began at {age}, in {first}"),
        )
    } else {
        (
            format!("at the end of {}", first - 1),
            format!("RMDs fall due from age {age}, in {first}"),
        )
    };
    let bank = ctx.main_bank();
    let fix = bank.map_or_else(
        || " The plan has no bank account to pay them into; add one first.".to_string(),
        |id| {
            format!(
                " A yearly Apply RMD event from age {age} pays each year's required amount \
                 into {}, taxed as ordinary income.",
                ctx.account_name(id)
            )
        },
    );
    let mut evidence: Vec<Evidence> = held
        .iter()
        .map(|(id, date, value)| Evidence::AccountSeries {
            account_id: *id,
            date: date.clone(),
            value: *value,
        })
        .collect();
    evidence.push(Evidence::Stat {
        name: "first RMD year".into(),
        value: first as f64,
    });
    let paths = bank
        .map(|bank| {
            let start_condition = (!past).then(|| json!({"kind": "Age", "years": age}));
            DraftPath::only(
                clip(&format!(
                    "Take yearly RMDs from age {age} into {}",
                    ctx.account_name(bank)
                )),
                vec![Change {
                    op: ChangeOp::Add,
                    target: ChangeTarget::NewEvent("rmd".into()),
                    path: String::new(),
                    expect: None,
                    value: Some(json!({
                        "name": "Required minimum distributions",
                        "fires_once": false,
                        "enabled": true,
                        "trigger": {
                            "kind": "Repeating",
                            "interval": "Yearly",
                            "start_condition": start_condition,
                            "end_condition": null,
                            "max_occurrences": null,
                        },
                        "effects": [{
                            "kind": "ApplyRmd",
                            "to_account_id": bank,
                            "lot_method": "Fifo",
                        }],
                    })),
                }],
            )
        })
        .into_iter()
        .collect();
    let s = if held.len() == 1 { "s" } else { "" };
    vec![Draft {
        rule: "rmd_missing",
        kind: Kind::Fix,
        section: Section::Plan,
        title: format!("{names} owe{s} RMDs from {first}, at {age}, but no event takes them"),
        summary: "FinPlan takes required minimum distributions only when an Apply RMD event \
                  runs, so this plan never withdraws or taxes them."
            .into(),
        reasoning: format!(
            "{names} hold{s} {total} on the median path {when}. {due} (born {birth}). No \
             enabled event applies them, so the money compounds untouched and the run \
             understates taxable income from then on.{fix}",
            total = money(total),
        ),
        evidence,
        paths,
    }]
}

/// An `ApplyRmd` that pays into an investment account: the distribution lands
/// there as cash, which no withdrawal spends and nothing invests unless an
/// event buys with it.
pub(super) fn rmd_into_investment_cash(ctx: &Ctx) -> Vec<Draft> {
    let first = ctx.birth_year().map(|birth| {
        let age = rmd_age(birth);
        (birth + i64::from(age), age)
    });
    let bank = ctx.main_bank();
    let mut drafts = Vec::new();
    for event in &ctx.events {
        for (i, effect) in event.effects.iter().enumerate() {
            let EffectSpec::ApplyRmd { to_account_id, .. } = effect else {
                continue;
            };
            if ctx.flavor(*to_account_id) != Some("Investment") {
                continue;
            }
            let account = ctx.account_name(*to_account_id);
            let from = first.map_or_else(String::new, |(year, age)| {
                format!(", from {year} at age {age}")
            });
            let mut evidence = Vec::new();
            if let Some((year, _)) = first {
                evidence.push(Evidence::Ledger {
                    year: ctx.start().map_or(year, |s| year.max(s.year)),
                    event_id: Some(event.id),
                    account_id: Some(*to_account_id),
                });
            }
            let mut fix = bank.map_or_else(
                || " The plan has no bank account to pay it into.".to_string(),
                |id| {
                    format!(
                        " Paying the RMD into {} puts it where expenses are paid from.",
                        ctx.account_name(id)
                    )
                },
            );
            // Two courses: pay the RMD where spending comes from (recommended
            // when there is a bank), or keep it here and invest it each year,
            // the bank holding the plan's cash buffer.
            let mut paths: Vec<DraftPath> = bank
                .map(|bank| {
                    DraftPath::single(
                        "a",
                        clip(&format!("Pay the RMD into {}", ctx.account_name(bank))),
                        true,
                        vec![Change {
                            op: ChangeOp::Replace,
                            target: ChangeTarget::Event(event.id),
                            path: format!("/effects/{i}/to_account_id"),
                            expect: Some(json!(to_account_id)),
                            value: Some(json!(bank)),
                        }],
                    )
                })
                .into_iter()
                .collect();
            if let Ok((_, changes)) = reinvest_path(ctx, *to_account_id, None) {
                fix.push_str(&format!(
                    " Or a yearly December 30 event invests the cash in {account}'s own \
                     holdings."
                ));
                paths.push(DraftPath::single(
                    "b",
                    clip(&format!("Invest the cash in {account} each year")),
                    paths.is_empty(),
                    changes,
                ));
            }
            drafts.push(Draft {
                rule: "rmd_into_investment_cash",
                kind: Kind::Check,
                section: Section::Plan,
                title: format!(
                    "{} pays RMDs into {account}, where they sit as cash",
                    event.name
                ),
                summary: format!(
                    "Cash in {account} is not invested unless an event buys with it, so the \
                     distributions sit idle until a withdrawal spends them."
                ),
                reasoning: format!(
                    "{}'s Apply RMD pays each year's distribution into {account}, an investment \
                     account{from}. The distribution is taxed as it leaves the pre-tax account, \
                     then sits in {account} as cash: withdrawals spend it before they sell \
                     anything, but until then it earns no investment return, and nothing \
                     invests it.{fix}",
                    event.name
                ),
                evidence,
                paths,
            });
        }
    }
    drafts
}

/// The bracket a conversion opportunity is measured against: the room below
/// the top of the 22% bracket is what the plan leaves unused.
const CONVERSION_CEILING: f64 = 0.22;

/// One plan year on the shown path, read for conversions.
struct TaxYear {
    year: i64,
    /// Ordinary income, with the year's RMD added where no event takes it.
    income: f64,
    /// The year's federal brackets, deduction folded in and indexed.
    brackets: Vec<TaxBracket>,
}

impl TaxYear {
    /// Ordinary income left before the top of the `rate` bracket: what
    /// `bracket_room(rate)` would read on December 30.
    fn room(&self, rate: f64) -> f64 {
        (bracket_ceiling(&self.brackets, rate) - self.income).max(0.0)
    }

    fn marginal(&self) -> f64 {
        marginal_rate(self.income, &self.brackets)
    }
}

/// The RMD due in `year` from `deferred` accounts, as the engine figures it:
/// each account's balance at the end of the year before over the IRS divisor
/// for the age reached that year. Zero past the table.
fn estimated_rmd(ctx: &Ctx, deferred: &[i64], birth: i64, year: i64) -> f64 {
    let Some(divisor) = u8::try_from(year - birth)
        .ok()
        .and_then(|age| RmdTable::irs_uniform_lifetime_2024().divisor_for_age(age))
    else {
        return 0.0;
    };
    deferred
        .iter()
        .filter_map(|id| ctx.year_end(*id, year - 1))
        .map(|balance| balance.max(0.0) / divisor)
        .sum()
}

/// The plan's investment accounts of one tax status, the largest at the
/// plan's start first (the first listed on a tie).
fn by_status(ctx: &Ctx, status: &str) -> Vec<i64> {
    let mut accounts: Vec<_> = ctx
        .graph
        .accounts
        .iter()
        .filter(|a| {
            ctx.graph
                .investment
                .get(&a.id)
                .is_some_and(|i| i.tax_status == status)
        })
        .collect();
    accounts.sort_by_key(|a| (a.sort_order, a.id));
    let mut valued: Vec<(i64, f64)> = accounts
        .into_iter()
        .map(|a| (a.id, ctx.opening_value(a.id)))
        .collect();
    // Stable: equals keep their list order.
    valued.sort_by(|a, b| b.1.total_cmp(&a.1));
    valued.into_iter().map(|(id, _)| id).collect()
}

/// A marginal rate as the Review tab writes it: `22%`, `12.5%`.
fn rate(rate: f64) -> String {
    let text = format!("{:.1}", rate * 100.0);
    format!("{}%", text.trim_end_matches('0').trim_end_matches('.'))
}

/// Pre-tax money whose RMDs fall due inside the plan, after full years whose
/// ordinary income leaves room below the top of the 22% bracket, when those
/// RMDs outrun spending or are taxed at a higher marginal rate than the years
/// before, and the plan converts nothing yet. The note carries the facts (the
/// room by year, the first RMD, the surplus, the rates and the tax) and the
/// `RothConversions` template's standard courses: convert each year up to the
/// 12% or the 22% bracket, tax paid from the largest taxable account, until
/// the year before RMDs.
///
/// Spec 21 has the rule detect and the AI reviewer write; this rule writes
/// the note itself, so a review without the model still shows it. The
/// reviewer reads it as ran and judges any conversion of its own on the
/// after-tax ending balance and lifetime tax its previews report.
pub(super) fn roth_conversion_opportunity(ctx: &Ctx) -> Vec<Draft> {
    let (Some(birth), Some(start), Some(end)) = (ctx.birth_year(), ctx.start(), ctx.end_year())
    else {
        return Vec::new();
    };
    let age = rmd_age(birth);
    let first = birth + i64::from(age);
    // RMDs already due leave no years before them; RMDs after the plan, no
    // tax to save inside it.
    if first <= start.year || first > end {
        return Vec::new();
    }
    let mut converts = false;
    let mut applies_rmd = false;
    for effect in ctx.events.iter().flat_map(|e| &e.effects) {
        visit(effect, &mut |e| match e {
            EffectSpec::RothConversion { .. } => converts = true,
            EffectSpec::ApplyRmd { .. } => applies_rmd = true,
            _ => {}
        });
    }
    if converts {
        return Vec::new();
    }
    let Some((from, from_date, from_value)) = deferred_at(ctx, first)
        .into_iter()
        .rev()
        .max_by(|a, b| a.2.total_cmp(&b.2))
    else {
        return Vec::new();
    };
    let Ok(tax) = crate::compile::tax_config(ctx.graph) else {
        return Vec::new();
    };
    let base = &tax.federal_brackets;
    if !bracket_ceiling(base, CONVERSION_CEILING).is_finite() {
        return Vec::new();
    }
    // The 65+ deduction applies from the tax year the filer turns 65, which
    // the engine counts from the day before the 65th birthday.
    let age_65_year = ctx
        .graph
        .scenario
        .birth_date
        .as_deref()
        .and_then(|d| d.parse::<jiff::civil::Date>().ok())
        .and_then(|d| d.yesterday().ok())
        .map(|d| i64::from(d.year()) + 65);
    let tax_year = |flow: &CashFlow, rmd: f64| {
        let extra = if age_65_year.is_some_and(|y| flow.year >= y) {
            tax.age_65_extra_deduction
        } else {
            0.0
        };
        TaxYear {
            year: flow.year,
            income: flow.ordinary_income + rmd,
            brackets: indexed_brackets(
                base,
                tax.standard_deduction + extra,
                ctx.inflation(flow.year),
            ),
        }
    };

    // The full years before RMDs that leave room below the 22% bracket.
    let flows = &ctx.results.cash_flows;
    let low: Vec<TaxYear> = flows
        .iter()
        .filter(|f| f.year < first && !ctx.is_partial_first_year(f.year))
        .map(|f| tax_year(f, 0.0))
        .filter(|y| y.room(CONVERSION_CEILING) >= 1.0)
        .collect();
    // The RMD years: each one's RMD, its taxes, and the RMD beyond spending.
    // RMDs are ordinary income; where no event takes them yet (`rmd_missing`
    // says so), they are added as the law will require.
    let deferred = by_status(ctx, "TaxDeferred");
    let rmd_years: Vec<(f64, TaxYear, f64)> = flows
        .iter()
        .filter(|f| (first..=end).contains(&f.year))
        .map(|f| {
            let rmd = estimated_rmd(ctx, &deferred, birth, f.year);
            let year = tax_year(f, if applies_rmd { 0.0 } else { rmd });
            (rmd, year, (rmd - f.expenses).max(0.0))
        })
        .collect();
    let (Some(low_first), Some(low_last)) = (low.first(), low.last()) else {
        return Vec::new();
    };
    let Some(&(first_rmd, ..)) = rmd_years.first().filter(|(rmd, ..)| *rmd >= 1.0) else {
        return Vec::new();
    };
    let low_rate = low.iter().map(TaxYear::marginal).fold(0.0, f64::max);
    let Some((peak_rate, peak_rate_year)) = rmd_years
        .iter()
        .rev()
        .map(|(_, y, _)| (y.marginal(), y.year))
        .max_by(|a, b| a.0.total_cmp(&b.0))
    else {
        return Vec::new();
    };
    let surplus: f64 = rmd_years.iter().map(|(.., s)| s).sum();
    let higher = peak_rate > low_rate + 1e-9;
    if surplus < 1.0 && !higher {
        return Vec::new();
    }

    // ── the facts ──
    let from_name = ctx.account_name(from);
    let room: f64 = low.iter().map(|y| y.room(CONVERSION_CEILING)).sum();
    let mut evidence = vec![Evidence::AccountSeries {
        account_id: from,
        date: from_date,
        value: from_value,
    }];
    evidence.extend(low.iter().map(|y| Evidence::Stat {
        name: format!("room below the top of the 22% bracket {}", y.year),
        value: y.room(CONVERSION_CEILING).round(),
    }));
    evidence.push(Evidence::Stat {
        name: "first RMD year".into(),
        value: first as f64,
    });
    evidence.push(Evidence::Stat {
        name: format!("RMD {first} (estimated)"),
        value: first_rmd.round(),
    });
    let peak_surplus = rmd_years
        .iter()
        .rev()
        .map(|(_, y, s)| (y.year, *s))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|(_, s)| *s >= 1.0);
    if let Some((year, s)) = peak_surplus {
        evidence.push(Evidence::Stat {
            name: format!("RMDs beyond spending {year}"),
            value: s.round(),
        });
        evidence.push(Evidence::Stat {
            name: format!("RMDs beyond spending {first}-{end}"),
            value: surplus.round(),
        });
    }
    evidence.push(Evidence::Stat {
        name: format!("marginal rate {peak_rate_year}"),
        value: peak_rate,
    });
    evidence.push(Evidence::Stat {
        name: format!("marginal rate {}-{}", low_first.year, low_last.year),
        value: low_rate,
    });
    let rmd_flows = || flows.iter().filter(|f| (first..=end).contains(&f.year));
    let rmd_tax: f64 = rmd_flows().map(|f| f.taxes).sum();
    if rmd_tax >= 1.0 {
        evidence.push(Evidence::Stat {
            name: format!("tax {first}-{end}"),
            value: rmd_tax.round(),
        });
    }
    // The year tax takes the most of spending.
    let peak_tax = rmd_flows()
        .filter(|f| f.expenses > 0.0 && f.taxes >= 1.0)
        .max_by(|a, b| (a.taxes / a.expenses).total_cmp(&(b.taxes / b.expenses)));
    if let Some(f) = peak_tax {
        evidence.push(Evidence::Ledger {
            year: f.year,
            event_id: None,
            account_id: None,
        });
    }

    // ── the courses ──
    let roth = by_status(ctx, "TaxFree").first().copied();
    // Opened beside the 401(k), its cash on the same profile.
    let open_roth = match roth {
        Some(_) => None,
        None => match ctx.graph.investment.get(&from) {
            Some(i) => Some(investment_account(
                "roth",
                "Roth IRA",
                "TaxFree",
                RowRef::Id(i.cash_return_profile_id),
                None,
                Vec::new(),
            )),
            None => return Vec::new(),
        },
    };
    let to = roth.map_or_else(|| RowRef::new("roth"), RowRef::Id);
    let to_name = roth.map_or_else(|| "a new Roth IRA".to_string(), |id| ctx.account_name(id));
    let payer = largest_taxable(ctx.graph).or_else(|| ctx.main_bank());
    let path = |key: &'static str, ceiling: f64, recommended: bool| -> Option<DraftPath> {
        let template = Template::RothConversions(RothConversionsParams {
            name: Some(format!("Roth conversions to {}", rate(ceiling))),
            from_account_id: RowRef::Id(from),
            to_account_id: to.clone(),
            ceiling_rate: ceiling,
            start: Some(When::Date {
                on_date: format!("{:04}-01-01", low_first.year),
            }),
            end: None,
            pay_tax_from_account_id: payer.map(RowRef::Id),
        });
        let changes = expand_template_in(ctx.graph, "", &template).ok()?.changes;
        let label = clip(&format!(
            "Convert {from_name} each year up to the {} bracket",
            rate(ceiling)
        ));
        let mut steps: Vec<DraftStep> = open_roth
            .iter()
            .map(|open| DraftStep {
                key: "a",
                title: "Open a Roth IRA to convert into".into(),
                reasoning: None,
                changes: vec![open.clone()],
            })
            .collect();
        steps.push(DraftStep {
            key: if steps.is_empty() { "a" } else { "b" },
            title: label.clone(),
            reasoning: None,
            changes,
        });
        Some(DraftPath {
            key,
            label,
            reasoning: Some(format!(
                "Fills up to {} of room below the top of the {} bracket from {} to {}, the \
                 year before RMDs.",
                money(low.iter().map(|y| y.room(ceiling)).sum()),
                rate(ceiling),
                low_first.year,
                first - 1
            )),
            recommended,
            steps,
        })
    };
    // Up to 22% when the RMD years are taxed above it, else the cheaper 12%.
    let has_12 = low.iter().any(|y| y.room(0.12) >= 1.0);
    let prefer_22 = peak_rate > CONVERSION_CEILING + 1e-9 || !has_12;
    let paths: Vec<DraftPath> = [
        has_12.then(|| path("12", 0.12, !prefer_22)).flatten(),
        path("22", CONVERSION_CEILING, prefer_22),
    ]
    .into_iter()
    .flatten()
    .collect();
    if paths.is_empty() {
        return Vec::new();
    }

    // ── the note ──
    let span = if low.len() as i64 == low_last.year - low_first.year + 1 {
        format!("the years {}-{}", low_first.year, low_last.year)
    } else {
        format!(
            "{} years from {} to {}",
            low.len(),
            low_first.year,
            low_last.year
        )
    };
    let rmd_note = if applies_rmd {
        ""
    } else {
        " No event takes them yet; these figures count them as the law requires."
    };
    let surplus_text = peak_surplus.map_or_else(String::new, |(year, s)| {
        format!(
            " RMDs outrun spending by {} over {first}-{end}, {} in {year} alone.",
            money(surplus),
            money(s)
        )
    });
    let rate_text = if higher {
        format!(
            " Their marginal rate reaches {} in {peak_rate_year}, against at most {} before.",
            rate(peak_rate),
            rate(low_rate)
        )
    } else {
        String::new()
    };
    let tax_text = peak_tax.map_or_else(String::new, |f| {
        format!(
            " Tax reaches {} in {}, {} of that year's spending.",
            money(f.taxes),
            f.year,
            percent(f.taxes / f.expenses)
        )
    });
    let payer_text = payer.map_or_else(
        || "withheld from each conversion".to_string(),
        |id| format!("paid from {}", ctx.account_name(id)),
    );
    let ladder = ctx
        .age_in(low_first.year)
        .filter(|age| *age < 60)
        .map_or_else(String::new, |age| {
            format!(
                " Converting from {age}, before 59½, also builds a ladder: each conversion \
                 can be withdrawn without penalty five years after it is made."
            )
        });
    let opens = if open_roth.is_some() {
        " The plan has no Roth account, so each path opens one first."
    } else {
        ""
    };
    vec![Draft {
        rule: "roth_conversion_opportunity",
        kind: Kind::Fix,
        section: Section::Plan,
        title: format!(
            "{from_name}'s RMDs from {first} ({}) follow years that leave {} below the 22% \
             bracket unused",
            money(first_rmd),
            money(room)
        ),
        summary: format!(
            "Converting pre-tax money to a Roth over {span} fills low brackets the plan \
             otherwise wastes, and shrinks RMDs taxed{} later.",
            if higher {
                format!(" at {}", rate(peak_rate))
            } else {
                String::new()
            }
        ),
        reasoning: format!(
            "On the median path, {span} leave ordinary income below the top of the 22% \
             bracket: {room} of room in all. {from_name} holds {held} at the end of {before}, \
             and RMDs begin at {age} in {first} (born {birth}): about {rmd} that year, its \
             prior year-end balance over the IRS divisor.{rmd_note}{surplus_text}{rate_text}\
             {tax_text} Converting each December 30 into {to_name}, tax {payer_text}, until \
             {before} moves that money out at the lower rates.{ladder}{opens} Judge it by the \
             after-tax ending balance and lifetime tax: success rarely moves. Not modeled: \
             IRMAA, ACA credits, the taxable share of Social Security, gains stacking on \
             ordinary income.",
            room = money(room),
            held = money(from_value),
            before = first - 1,
            rmd = money(first_rmd),
        ),
        evidence,
        paths,
    }]
}
