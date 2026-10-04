//! Notes on events: effects that do something other than what they appear to.

use serde_json::json;

use super::portfolio::visit;
use super::{
    Ctx, Draft, DraftPath, Evidence, Kind, Section, clip, fixed_amount, list, money, rmd_age,
    year_of,
};
use crate::specs::{AmountSpec, EffectSpec, Interval, TriggerSpec};
use crate::suggest::{Change, ChangeOp, ChangeTarget};

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
            let fix = bank.map_or_else(
                || " The plan has no bank account to pay it into.".to_string(),
                |id| {
                    format!(
                        " Paying the RMD into {} puts it where expenses are paid from.",
                        ctx.account_name(id)
                    )
                },
            );
            // Retargeting to the bank is the one path for now; reinvesting the
            // cash in place joins it once the `ReinvestCash` template exists
            // (spec 21, with `cash_accumulates`).
            let paths = bank
                .map(|bank| {
                    DraftPath::only(
                        clip(&format!("Pay the RMD into {}", ctx.account_name(bank))),
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
            drafts.push(Draft {
                rule: "rmd_into_investment_cash",
                kind: Kind::Check,
                section: Section::Plan,
                title: format!(
                    "{} pays RMDs into {account}, where they sit as cash",
                    event.name
                ),
                summary: format!(
                    "Cash in {account} is not spent by withdrawals or invested unless an event \
                     buys with it, so the distributions pile up idle."
                ),
                reasoning: format!(
                    "{}'s Apply RMD pays each year's distribution into {account}, an investment \
                     account{from}. The distribution is taxed as it leaves the pre-tax account, \
                     then sits in {account} as cash: sweeps sell holdings, not cash, so \
                     spending never draws on it, and nothing invests it.{fix}",
                    event.name
                ),
                evidence,
                paths,
            });
        }
    }
    drafts
}
