//! Tests for contribution limits
//!
//! Tests that contribution limits are enforced correctly for monthly and yearly periods.

use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountFlavor, AccountId, AmountMode, AssetId, Cash, ContributionLimit,
    ContributionLimitPeriod, Event, EventEffect, EventId, EventTrigger, IncomeType,
    InflationProfile, InvestmentContainer, RepeatInterval, ReturnProfile, ReturnProfileId,
    TaxStatus, TransferAmount,
};
use crate::simulation::simulate;

#[test]
fn test_monthly_contribution_limit() {
    // Test that monthly contribution limits are enforced and reset each month
    let roth_ira = AccountId(1);
    let start_date = jiff::civil::date(2024, 1, 1);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 1,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([(ReturnProfileId(0), ReturnProfile::Fixed(0.0))]),
        asset_returns: HashMap::new(),
        accounts: vec![Account {
            account_id: roth_ira,
            flavor: AccountFlavor::Investment(InvestmentContainer {
                tax_status: TaxStatus::TaxFree,
                cash: Cash {
                    value: 0.0,
                    return_profile_id: ReturnProfileId(0),
                },
                positions: vec![],
                contribution_limit: Some(ContributionLimit {
                    amount: 500.0, // $500/month limit
                    period: ContributionLimitPeriod::Monthly,
                    catch_up: Vec::new(),
                }),
            }),
        }],
        events: vec![
            // Try to contribute $600 on Jan 15 (should be capped at $500)
            Event {
                event_id: EventId(1),
                trigger: EventTrigger::Date(jiff::civil::date(2024, 1, 15)),
                effects: vec![EventEffect::Income {
                    to: roth_ira,
                    amount: TransferAmount::fixed(600.0),
                    amount_mode: AmountMode::Net,
                    income_type: IncomeType::TaxFree,
                }],
                once: true,
            },
            // Try to contribute another $200 on Jan 25 (should be blocked - limit reached)
            Event {
                event_id: EventId(2),
                trigger: EventTrigger::Date(jiff::civil::date(2024, 1, 25)),
                effects: vec![EventEffect::Income {
                    to: roth_ira,
                    amount: TransferAmount::fixed(200.0),
                    amount_mode: AmountMode::Net,
                    income_type: IncomeType::TaxFree,
                }],
                once: true,
            },
            // Contribute $500 on Feb 1 (should succeed - new month)
            Event {
                event_id: EventId(3),
                trigger: EventTrigger::Date(jiff::civil::date(2024, 2, 1)),
                effects: vec![EventEffect::Income {
                    to: roth_ira,
                    amount: TransferAmount::fixed(500.0),
                    amount_mode: AmountMode::Net,
                    income_type: IncomeType::TaxFree,
                }],
                once: true,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();

    // Check final balance - should be $1000 ($500 from Jan + $500 from Feb)
    let final_balance = result.final_account_balance(roth_ira).unwrap();
    assert!(
        (final_balance - 1000.0).abs() < 0.01,
        "Expected $1000, got ${final_balance:.2}"
    );

    // Verify ledger entries
    let cash_credits: Vec<_> = result
        .ledger
        .iter()
        .filter_map(|entry| match &entry.event {
            crate::model::StateEvent::CashCredit { to, amount, .. } if *to == roth_ira => {
                Some((*to, *amount))
            }
            _ => None,
        })
        .collect();

    assert_eq!(
        cash_credits.len(),
        2,
        "Should have exactly 2 cash credits (Jan and Feb)"
    );
    assert!(
        (cash_credits[0].1 - 500.0).abs() < 0.01,
        "First credit should be $500, got ${:.2}",
        cash_credits[0].1
    );
    assert!(
        (cash_credits[1].1 - 500.0).abs() < 0.01,
        "Second credit should be $500, got ${:.2}",
        cash_credits[1].1
    );
}

#[test]
fn test_yearly_contribution_limit() {
    // Test that yearly contribution limits are enforced across multiple months
    let roth_401k = AccountId(1);
    let start_date = jiff::civil::date(2024, 1, 1);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 2,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([(ReturnProfileId(0), ReturnProfile::Fixed(0.0))]),
        asset_returns: HashMap::new(),
        accounts: vec![Account {
            account_id: roth_401k,
            flavor: AccountFlavor::Investment(InvestmentContainer {
                tax_status: TaxStatus::TaxFree,
                cash: Cash {
                    value: 0.0,
                    return_profile_id: ReturnProfileId(0),
                },
                positions: vec![],
                contribution_limit: Some(ContributionLimit {
                    amount: 23000.0, // $23k/year limit (2024 Roth 401k limit)
                    period: ContributionLimitPeriod::Yearly,
                    catch_up: Vec::new(),
                }),
            }),
        }],
        events: vec![
            // Monthly contributions of $2000 repeating
            Event {
                event_id: EventId(1),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Monthly,
                    start_condition: Some(Box::new(EventTrigger::Date(start_date))),
                    end_condition: None,
                    max_occurrences: None,
                },
                effects: vec![EventEffect::Income {
                    to: roth_401k,
                    amount: TransferAmount::fixed(2000.0),
                    amount_mode: AmountMode::Net,
                    income_type: IncomeType::TaxFree,
                }],
                once: false,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();

    // Check final balance
    // First year: 12 months * $2000 = $24000, but capped at $23000
    // Second year: 12 months * $2000 = $24000, but capped at $23000
    // Total: $46000
    let final_balance = result.final_account_balance(roth_401k).unwrap();
    assert!(
        (final_balance - 46000.0).abs() < 0.01,
        "Expected $46000, got ${final_balance:.2}. Should be capped at $23k per year for 2 years."
    );

    // Count contributions per year
    let mut year_2024_total = 0.0;
    let mut year_2025_total = 0.0;

    for entry in &result.ledger {
        if let crate::model::StateEvent::CashCredit { to, amount, .. } = &entry.event
            && *to == roth_401k
        {
            if entry.date.year() == 2024 {
                year_2024_total += amount;
            } else if entry.date.year() == 2025 {
                year_2025_total += amount;
            }
        }
    }

    assert!(
        (year_2024_total - 23000.0).abs() < 0.01,
        "2024 contributions should be capped at $23000, got ${year_2024_total:.2}"
    );
    assert!(
        (year_2025_total - 23000.0).abs() < 0.01,
        "2025 contributions should be capped at $23000, got ${year_2025_total:.2}"
    );
}

#[test]
fn test_contribution_limit_with_asset_purchase() {
    // Test that contribution limits apply to asset purchases as well
    let ira = AccountId(1);
    let checking = AccountId(2);
    let vtsax = AssetId(1);
    let start_date = jiff::civil::date(2024, 1, 1);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 1,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([(ReturnProfileId(0), ReturnProfile::Fixed(0.0))]),
        asset_returns: HashMap::from([(vtsax, ReturnProfileId(0))]),
        asset_prices: HashMap::from([(vtsax, 100.0)]), // $100 per share
        accounts: vec![
            Account {
                account_id: ira,
                flavor: AccountFlavor::Investment(InvestmentContainer {
                    tax_status: TaxStatus::TaxDeferred,
                    cash: Cash {
                        value: 0.0,
                        return_profile_id: ReturnProfileId(0),
                    },
                    positions: vec![],
                    contribution_limit: Some(ContributionLimit {
                        amount: 7000.0, // $7k/year IRA limit
                        period: ContributionLimitPeriod::Yearly,
                        catch_up: Vec::new(),
                    }),
                }),
            },
            Account {
                account_id: checking,
                flavor: AccountFlavor::Bank(Cash {
                    value: 50000.0,
                    return_profile_id: ReturnProfileId(0),
                }),
            },
        ],
        events: vec![
            // Add $3000 cash to IRA
            Event {
                event_id: EventId(1),
                trigger: EventTrigger::Date(jiff::civil::date(2024, 1, 15)),
                effects: vec![EventEffect::Income {
                    to: ira,
                    amount: TransferAmount::fixed(3000.0),
                    amount_mode: AmountMode::Net,
                    income_type: IncomeType::TaxFree,
                }],
                once: true,
            },
            // Purchase $2000 of VTSAX in IRA from IRA cash (doesn't count against limit)
            Event {
                event_id: EventId(2),
                trigger: EventTrigger::Date(jiff::civil::date(2024, 2, 1)),
                effects: vec![EventEffect::AssetPurchase {
                    from: ira,
                    to: crate::model::AssetCoord {
                        account_id: ira,
                        asset_id: vtsax,
                    },
                    amount: TransferAmount::fixed(2000.0),
                }],
                once: true,
            },
            // Try to add another $5000 (should be capped at $4000 remaining limit)
            Event {
                event_id: EventId(3),
                trigger: EventTrigger::Date(jiff::civil::date(2024, 3, 1)),
                effects: vec![EventEffect::Income {
                    to: ira,
                    amount: TransferAmount::fixed(5000.0),
                    amount_mode: AmountMode::Net,
                    income_type: IncomeType::TaxFree,
                }],
                once: true,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();

    // Total contributions should be capped at $7000 ($3000 + $4000)
    let final_balance = result.final_account_balance(ira).unwrap();
    assert!(
        (final_balance - 7000.0).abs() < 0.01,
        "Expected $7000 total value, got ${final_balance:.2}"
    );

    // Verify cash and asset distribution
    // $3000 initial + $4000 second = $7000 total, minus $2000 for asset purchase = $5000 cash
    let mut ira_cash = 0.0;
    for entry in &result.ledger {
        match &entry.event {
            crate::model::StateEvent::CashCredit { to, amount, .. } if *to == ira => {
                ira_cash += amount;
            }
            crate::model::StateEvent::CashDebit { from, amount, .. } if *from == ira => {
                ira_cash -= amount;
            }
            _ => {}
        }
    }

    assert!(
        (ira_cash - 5000.0).abs() < 0.01,
        "Expected $5000 cash remaining, got ${ira_cash:.2}"
    );

    // Check asset balance ($2000 worth / $100 per share = 20 shares)
    let asset_balance = result.final_asset_balance(ira, vtsax).unwrap_or(0.0);

    assert!(
        (asset_balance - 2000.0).abs() < 0.01,
        "Expected $2000 in assets, got ${asset_balance:.2}"
    );
}

/// Contribute $5,000 a month to a capped 401(k) for someone born on
/// `birth_date`, and return the amount credited in each calendar year.
fn yearly_credits_with_catch_up(
    birth_date: jiff::civil::Date,
    start_year: i16,
    years: usize,
) -> Vec<(i16, f64)> {
    let k401 = AccountId(1);
    let start_date = jiff::civil::date(start_year, 1, 1);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: years,
        birth_date: Some(birth_date),
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([(ReturnProfileId(0), ReturnProfile::Fixed(0.0))]),
        accounts: vec![Account {
            account_id: k401,
            flavor: AccountFlavor::Investment(InvestmentContainer {
                tax_status: TaxStatus::TaxDeferred,
                cash: Cash {
                    value: 0.0,
                    return_profile_id: ReturnProfileId(0),
                },
                positions: vec![],
                contribution_limit: Some(
                    ContributionLimit::yearly(23_000.0)
                        .with_catch_up(50, None, 7_500.0)
                        .with_catch_up(60, Some(63), 11_250.0),
                ),
            }),
        }],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Monthly,
                start_condition: Some(Box::new(EventTrigger::Date(start_date))),
                end_condition: None,
                max_occurrences: None,
            },
            effects: vec![EventEffect::Income {
                to: k401,
                amount: TransferAmount::fixed(5_000.0),
                amount_mode: AmountMode::Net,
                income_type: IncomeType::TaxFree,
            }],
            once: false,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42).unwrap();

    let mut totals: Vec<(i16, f64)> = (0..years as i16).map(|i| (start_year + i, 0.0)).collect();
    for entry in &result.ledger {
        if let crate::model::StateEvent::CashCredit { to, amount, .. } = &entry.event
            && *to == k401
            && let Some(slot) = totals.iter_mut().find(|(y, _)| *y == entry.date.year())
        {
            slot.1 += amount;
        }
    }
    totals
}

fn assert_yearly(actual: &[(i16, f64)], expected: &[(i16, f64)]) {
    for ((year, got), (_, want)) in actual.iter().zip(expected) {
        assert!(
            (got - want).abs() < 0.01,
            "{year}: expected ${want:.2}, got ${got:.2} (all years: {actual:?})"
        );
    }
}

#[test]
fn test_catch_up_starts_in_the_year_of_the_50th_birthday() {
    // Turns 50 on Dec 15, 2025: the catch-up is open from Jan 1, 2025.
    let totals = yearly_credits_with_catch_up(jiff::civil::date(1975, 12, 15), 2024, 2);
    assert_yearly(&totals, &[(2024, 23_000.0), (2025, 30_500.0)]);
}

#[test]
fn test_age_60_to_63_catch_up_replaces_the_age_50_one() {
    // Born mid-1963: 59 in 2022, 60-63 in 2023-2026, 64 in 2027.
    let totals = yearly_credits_with_catch_up(jiff::civil::date(1963, 6, 1), 2022, 6);
    assert_yearly(
        &totals,
        &[
            (2022, 30_500.0),
            (2023, 34_250.0),
            (2024, 34_250.0),
            (2025, 34_250.0),
            (2026, 34_250.0),
            (2027, 30_500.0),
        ],
    );
}

#[test]
fn test_catch_up_tiers_take_the_largest_not_the_sum() {
    let limit = ContributionLimit::yearly(23_000.0)
        .with_catch_up(50, None, 7_500.0)
        .with_catch_up(60, Some(63), 11_250.0);
    assert!((limit.amount_for_age(49) - 23_000.0).abs() < f64::EPSILON);
    assert!((limit.amount_for_age(50) - 30_500.0).abs() < f64::EPSILON);
    assert!((limit.amount_for_age(60) - 34_250.0).abs() < f64::EPSILON);
    assert!((limit.amount_for_age(63) - 34_250.0).abs() < f64::EPSILON);
    assert!((limit.amount_for_age(64) - 30_500.0).abs() < f64::EPSILON);
}
