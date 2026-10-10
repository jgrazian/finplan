//! Spending rules (spec 22): fixed retirement spending a rule would carry
//! through bad markets, and a rule whose bad markets cut too deep.

use crate::specs::{AmountSpec, EffectSpec, Interval, SpendingRuleSpec, TriggerSpec};
use crate::suggest::{Change, ChangeOp, ChangeTarget};

use super::{Ctx, Draft, DraftPath, Evidence, Kind, Section, clip, fixed_amount, money, percent};

/// A first-year withdrawal rate above this, with funding success below
/// [`STRESSED_FUNDING`], is fixed spending under stress.
const STRESSED_RATE: f64 = 0.045;
const STRESSED_FUNDING: f64 = 0.85;
/// A rule whose 10th-percentile lowest draw is under this share of its first
/// draw cuts deep in bad markets.
const DEEP_CUT: f64 = 0.5;
/// The floor offered, as a share of the first draw.
const FLOOR_SHARE: f64 = 2.0 / 3.0;

fn per_year(trigger: &TriggerSpec) -> Option<f64> {
    match trigger {
        TriggerSpec::Repeating { interval, .. } => match interval {
            Interval::Monthly => Some(12.0),
            Interval::Quarterly => Some(4.0),
            Interval::Yearly => Some(1.0),
            _ => None,
        },
        _ => None,
    }
}

/// An amount in today's dollars per occurrence: a figure, raised for
/// inflation or not, written as an amount tree or as `inflation(N)` / `N`.
fn todays_amount(amount: &AmountSpec) -> Option<f64> {
    if let Some((value, _)) = fixed_amount(amount, 1.0) {
        return Some(value);
    }
    let AmountSpec::Expression { source } = amount else {
        return None;
    };
    let text = source.trim();
    let inner = text
        .strip_prefix("inflation(")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(text);
    inner.trim().parse::<f64>().ok()
}

/// What the plan's liquid accounts held, nominal, at the last balance before
/// `date` on the shown path.
fn liquid_before(ctx: &Ctx, date: &str) -> f64 {
    ctx.graph
        .accounts
        .iter()
        .filter(|a| a.flavor == "Bank" || a.flavor == "Investment")
        .filter_map(|a| ctx.balance_before(a.id, date).map(|(_, v)| v))
        .sum()
}

/// Fixed retirement spending above a 4.5% first-year withdrawal rate, in a
/// plan that funds itself in under 85% of markets: Guardrails at the same
/// starting amount would cut in the bad markets instead of running short.
pub(super) fn fixed_spending_stress(ctx: &Ctx) -> Vec<Draft> {
    let Some(funding) = ctx.results.stats.funding_success_rate else {
        return Vec::new();
    };
    if funding >= STRESSED_FUNDING {
        return Vec::new();
    }
    // A plan that already spends by a rule has made this choice.
    if ctx
        .events
        .iter()
        .flat_map(|e| &e.effects)
        .any(|e| matches!(e, EffectSpec::Spending { .. }))
    {
        return Vec::new();
    }
    let mut drafts = Vec::new();
    for event in ctx
        .events
        .iter()
        .filter(|e| e.name.to_lowercase().contains("retire"))
    {
        let Some(per_year) = per_year(&event.trigger) else {
            continue;
        };
        let Some((index, from, amount)) =
            event.effects.iter().enumerate().find_map(|(i, e)| match e {
                EffectSpec::Expense {
                    from_account_id,
                    amount,
                } => Some((i, *from_account_id, amount)),
                _ => None,
            })
        else {
            continue;
        };
        let Some(each) = todays_amount(amount) else {
            continue;
        };
        let Some(start) = ctx.fires(&event.trigger) else {
            continue;
        };
        let yearly = each * per_year;
        let portfolio = liquid_before(ctx, &start.date());
        if portfolio <= 0.0 {
            continue;
        }
        let rate = yearly * ctx.inflation(start.year) / portfolio;
        if rate <= STRESSED_RATE {
            continue;
        }
        let Ok(expect) = serde_json::to_value(&event.effects[index]) else {
            continue;
        };
        let replacement = EffectSpec::Spending {
            from_account_id: from,
            rule: SpendingRuleSpec::guardrails(yearly.round()),
            portfolio: crate::specs::SpendingPortfolioSpec::Liquid,
            pass_through: true,
        };
        let Ok(value) = serde_json::to_value(&replacement) else {
            continue;
        };
        drafts.push(Draft {
            rule: "fixed_spending_stress",
            kind: Kind::Fix,
            section: Section::Plan,
            title: clip(&format!(
                "{} draws {} of the portfolio at retirement, fixed whatever markets do",
                event.name,
                percent(rate)
            )),
            summary: format!(
                "Spending {} a year that never adjusts is funded in {} of markets; guardrails \
                 at the same start would cut in the bad ones instead of running short.",
                money(yearly),
                percent(funding)
            ),
            reasoning: format!(
                "{} spends {} a year in today's dollars from {}, {} of the liquid portfolio on \
                 the median path when it starts in {}. Fixed spending has to survive the worst \
                 markets without changing, and here cash runs short in {} of them. Guardrails \
                 start at the same amount, cut 10% when withdrawals grow 20% past where they \
                 started and raise 10% when they fall 20% below; the preview shows what that \
                 does to funding success and how deep the cuts go. Income landing in {} is \
                 spent on top of the draw.",
                event.name,
                money(yearly),
                ctx.account_name(from),
                percent(rate),
                start.year,
                percent(1.0 - funding),
                ctx.account_name(from)
            ),
            evidence: vec![
                Evidence::Stat {
                    name: "first-year withdrawal rate".into(),
                    value: rate,
                },
                Evidence::Stat {
                    name: "funding success rate".into(),
                    value: funding,
                },
            ],
            paths: vec![DraftPath::only(
                clip(&format!(
                    "Spend by guardrails from {} a year",
                    money(yearly)
                )),
                vec![Change {
                    op: ChangeOp::Replace,
                    target: ChangeTarget::Event(event.id),
                    path: format!("/effects/{index}"),
                    expect: Some(expect),
                    value: Some(value),
                }],
            )],
        });
    }
    drafts
}

/// A rule without a floor whose bad markets take the draw below half of
/// where it started: offer a floor.
pub(super) fn spending_rule_no_floor(ctx: &Ctx) -> Vec<Draft> {
    let Some(spending) = &ctx.results.spending else {
        return Vec::new();
    };
    let first = spending.median_first_draw;
    if first <= 0.0 || spending.p10_lowest_draw >= DEEP_CUT * first {
        return Vec::new();
    }
    let mut drafts = Vec::new();
    for event in &ctx.events {
        for (index, effect) in event.effects.iter().enumerate() {
            let EffectSpec::Spending { rule, .. } = effect else {
                continue;
            };
            let floor = match rule {
                SpendingRuleSpec::Guardrails { floor, .. }
                | SpendingRuleSpec::PercentOfPortfolio { floor, .. } => floor,
                _ => continue,
            };
            if floor.is_some() {
                continue;
            }
            let offered = ((first * FLOOR_SHARE) / 1000.0).round() * 1000.0;
            drafts.push(Draft {
                rule: "spending_rule_no_floor",
                kind: Kind::Check,
                section: Section::Plan,
                title: clip(&format!(
                    "In 1 market in 10, {}'s draw falls to {} a year, from {}",
                    event.name,
                    money(spending.p10_lowest_draw),
                    money(first)
                )),
                summary: "The rule has no floor, so in bad markets it cuts below half of where \
                          it started; a floor holds spending up at the cost of drawing harder \
                          on the portfolio."
                    .into(),
                reasoning: format!(
                    "{}'s {} rule starts at a median {} a year in today's dollars, and in the \
                     worst tenth of markets its lowest year is {}. A floor of {} would keep the \
                     draw from going below it; it can also run the portfolio out sooner, which \
                     the preview's funding success shows.",
                    event.name,
                    rule.label(),
                    money(first),
                    money(spending.p10_lowest_draw),
                    money(offered)
                ),
                evidence: vec![
                    Evidence::Stat {
                        name: "median first-year draw".into(),
                        value: first,
                    },
                    Evidence::Stat {
                        name: "P10 lowest draw".into(),
                        value: spending.p10_lowest_draw,
                    },
                ],
                paths: vec![DraftPath::only(
                    clip(&format!("Set a floor of {} a year", money(offered))),
                    vec![Change {
                        op: ChangeOp::Replace,
                        target: ChangeTarget::Event(event.id),
                        path: format!("/effects/{index}/rule/floor"),
                        expect: Some(serde_json::Value::Null),
                        value: Some(serde_json::json!(offered)),
                    }],
                )],
            });
        }
    }
    drafts
}
