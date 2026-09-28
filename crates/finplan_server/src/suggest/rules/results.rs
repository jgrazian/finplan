//! Readings of the run: where and why paths fail, and what the headline
//! number does and doesn't count.

use super::{Ctx, Draft, Evidence, Kind, Section, money, percent, thousands};

/// Share of shortfall paths one account must account for before the note
/// names it as the account that empties.
const CONCENTRATION: f64 = 0.5;
/// Share of failing paths that must end solvent before failures are called a
/// liquidity problem.
const SOLVENT_SHARE: f64 = 0.5;

/// Failures concentrated in one account, or failing paths that still end
/// with money: both say the plan runs short of spendable cash, not wealth.
pub(super) fn shortfall_account_concentration(ctx: &Ctx) -> Vec<Draft> {
    let Some(fd) = &ctx.results.funding_diagnostics else {
        return Vec::new();
    };
    if fd.failed == 0 {
        return Vec::new();
    }
    let top = fd
        .shortfall_accounts
        .first()
        .filter(|a| {
            fd.cash_shortfall > 0 && a.count as f64 >= CONCENTRATION * fd.cash_shortfall as f64
        })
        .and_then(|a| Some((a.account_id?, a.count)));
    let solvent = fd.failed_solvent as f64 >= SOLVENT_SHARE * fd.failed as f64;
    if top.is_none() && !solvent {
        return Vec::new();
    }

    let mut evidence = vec![
        Evidence::Diagnostic {
            field: "failed".into(),
            value: fd.failed as f64,
        },
        Evidence::Diagnostic {
            field: "cash_shortfall".into(),
            value: fd.cash_shortfall as f64,
        },
    ];
    let mut sentences = vec![format!(
        "{} of {} iterations failed the funding check.",
        thousands(fd.failed),
        thousands(fd.iterations)
    )];
    if let Some((account_id, count)) = top {
        sentences.push(format!(
            "In {count} of the {} that ran short of cash, {} was the account overdrawn first.",
            fd.cash_shortfall,
            ctx.account_name(account_id)
        ));
        evidence.push(Evidence::Diagnostic {
            field: "shortfall_accounts".into(),
            value: count as f64,
        });
    }
    if let Some(year) = fd.median_first_shortfall_year {
        let age = ctx
            .age_in(year)
            .map(|age| format!(", at age {age}"))
            .unwrap_or_default();
        sentences.push(format!("The median first shortfall is in {year}{age}."));
        evidence.push(Evidence::Diagnostic {
            field: "median_first_shortfall_year".into(),
            value: year as f64,
        });
    }
    if let (Some(deficit), Some(years)) = (fd.median_max_deficit, fd.median_shortfall_years) {
        sentences.push(format!(
            "A typical shortfall path is short by up to {} and stays short for {years:.0} years.",
            money(deficit)
        ));
    }
    if solvent {
        sentences.push(format!(
            "{} of the {} failing paths still end with positive net worth: the plan runs out of \
             spendable cash before it runs out of money, which is a funding rule to fix rather \
             than too little wealth.",
            fd.failed_solvent, fd.failed
        ));
        evidence.push(Evidence::Diagnostic {
            field: "failed_solvent".into(),
            value: fd.failed_solvent as f64,
        });
    }

    let title = match top {
        Some((account_id, count)) => format!(
            "{} is the account that runs short in {count} of {} shortfall paths",
            ctx.account_name(account_id),
            fd.cash_shortfall
        ),
        None => format!(
            "{} of {} failing paths still end with money: liquidity, not wealth",
            fd.failed_solvent, fd.failed
        ),
    };
    vec![Draft {
        rule: "shortfall_account_concentration",
        kind: Kind::Read,
        section: Section::Results,
        title,
        reasoning: sentences.join(" "),
        evidence,
        paths: Vec::new(),
    }]
}

/// Success rate at least this far above funding success before the gap is
/// worth a note.
const GAP: f64 = 0.02;
/// Property share of the shown path's final net worth that counts as propping
/// it up.
const PROPERTY_SHARE: f64 = 0.25;

/// The success rate counts paths that end with positive net worth, including
/// illiquid property; say so when that flatters it.
pub(super) fn success_vs_funding_gap(ctx: &Ctx) -> Vec<Draft> {
    let stats = &ctx.results.stats;
    let gap = stats
        .funding_success_rate
        .map(|funded| (funded, stats.success_rate - funded))
        .filter(|(_, gap)| *gap >= GAP);

    // Property on the shown path at its last point, against that path's net worth.
    let final_net_worth = ctx
        .results
        .bands
        .iter()
        .find(|band| band.path_id == ctx.results.series_id)
        .and_then(|band| band.net_worth.last().copied());
    let mut property: Vec<(i64, f64)> = ctx
        .graph
        .property
        .keys()
        .filter_map(|id| {
            let last = ctx.balances(*id).last()?.1;
            (last > 0.0).then_some((*id, last))
        })
        .collect();
    property.sort_by_key(|(id, _)| *id);
    let property_total: f64 = property.iter().map(|(_, v)| v).sum();
    let share = final_net_worth
        .filter(|nw| *nw > 0.0)
        .map(|nw| property_total / nw)
        .filter(|share| *share >= PROPERTY_SHARE);

    if gap.is_none() && share.is_none() {
        return Vec::new();
    }
    let names: Vec<String> = property
        .iter()
        .map(|(id, _)| ctx.account_name(*id))
        .collect();
    let mut evidence = vec![Evidence::Stat {
        name: "success_rate".into(),
        value: stats.success_rate,
    }];
    let mut sentences = vec![
        "Success counts every path that ends with positive net worth, whatever happened on the way."
            .to_string(),
    ];
    let title = if let Some((funded, gap)) = gap {
        let paths = (gap * stats.num_iterations as f64).round();
        sentences.push(format!(
            "{paths:.0} paths ({:.1} points) end positive yet ran short of cash along the way, \
             which the funding check counts as failures.",
            gap * 100.0
        ));
        evidence.push(Evidence::Stat {
            name: "funding_success_rate".into(),
            value: funded,
        });
        format!(
            "Success says {}, but {} of paths stayed funded",
            percent(stats.success_rate),
            percent(funded)
        )
    } else {
        format!(
            "{} is {} of the median path's final net worth",
            super::list(&names),
            percent(share.unwrap_or(0.0))
        )
    };
    if let Some(share) = share {
        sentences.push(format!(
            "Property counts toward that net worth: {} is {} at the end of the median path, {} \
             of it, and cannot pay a bill without being sold.",
            super::list(&names),
            money(property_total),
            percent(share)
        ));
        evidence.push(Evidence::Stat {
            name: "property share of final net worth".into(),
            value: share,
        });
    }
    vec![Draft {
        rule: "success_vs_funding_gap",
        kind: Kind::Read,
        section: Section::Results,
        title,
        reasoning: sentences.join(" "),
        evidence,
        paths: Vec::new(),
    }]
}
