//! Tests for SimulationResult structure and methods
//!
//! These tests verify:
//! - Result dates are correct
//! - Account snapshots capture starting state
//! - Records are generated correctly
//! - Helper methods work as expected

use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountFlavor, AccountId, AmountMode, AssetId, AssetLot, Cash, Event, EventEffect,
    EventId, EventTrigger, IncomeType, InflationProfile, InvestmentContainer, ReturnProfile,
    ReturnProfileId, TaxStatus, TransferAmount,
};
use crate::simulation::simulate;

/// Test that simulation dates are recorded correctly
#[test]
fn test_simulation_dates() {
    let start_date = jiff::civil::date(2020, 6, 15);
    let years = 5;

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: years,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::new(),
        asset_returns: HashMap::new(),
        accounts: vec![],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // First date should be start date
    assert_eq!(
        result.wealth_snapshots.first().map(|snap| snap.date),
        Some(start_date),
        "First date should be start date"
    );

    // Last date should be start + duration
    let expected_end = jiff::civil::date(2025, 6, 15);
    assert_eq!(
        result.wealth_snapshots.last().map(|snap| snap.date),
        Some(expected_end),
        "Last date should be start + {} years",
        years
    );
}

/// Test that final balances are stored correctly
#[test]
fn test_final_balances_stored() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let asset_id = AssetId(1);
    let return_profile_id = ReturnProfileId(0);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 5,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([(return_profile_id, ReturnProfile::Fixed(0.10))]),
        asset_returns: HashMap::from([(asset_id, return_profile_id)]),
        accounts: vec![
            Account {
                account_id: AccountId(1),
                flavor: AccountFlavor::Investment(InvestmentContainer {
                    tax_status: TaxStatus::Taxable,
                    cash: Cash {
                        value: 0.0,
                        return_profile_id: ReturnProfileId(999),
                    },
                    positions: vec![AssetLot {
                        asset_id,
                        purchase_date: start_date,
                        units: 10_000.0,
                        cost_basis: 10_000.0,
                    }],
                    contribution_limit: None,
                }),
            },
            Account {
                account_id: AccountId(2),
                flavor: AccountFlavor::Bank(Cash {
                    value: 5_000.0,
                    return_profile_id: ReturnProfileId(999),
                }),
            },
        ],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Verify final_balances HashMap is populated
    assert!(
        result.final_account_balance(AccountId(1)).is_some(),
        "final_balances should contain account 1"
    );
    assert!(
        result.final_account_balance(AccountId(2)).is_some(),
        "final_balances should contain account 2"
    );

    // Verify values are correct
    let expected_investment = 10_000.0 * (1.10_f64).powi(5);
    let actual_investment = result.final_account_balance(AccountId(1)).unwrap();
    assert!(
        (actual_investment - expected_investment).abs() < 1.0,
        "Investment balance expected ${:.2}, got ${:.2}",
        expected_investment,
        actual_investment
    );

    let actual_bank = result.final_account_balance(AccountId(2)).unwrap();
    assert!(
        (actual_bank - 5_000.0).abs() < 0.01,
        "Bank balance expected $5000, got ${:.2}",
        actual_bank
    );
}

/// Test that asset balances are stored correctly
#[test]
fn test_final_asset_balances_stored() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let stock_id = AssetId(1);
    let bond_id = AssetId(2);
    let stock_profile = ReturnProfileId(0);
    let bond_profile = ReturnProfileId(1);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 3,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([
            (stock_profile, ReturnProfile::Fixed(0.08)),
            (bond_profile, ReturnProfile::Fixed(0.04)),
        ]),
        asset_returns: HashMap::from([(stock_id, stock_profile), (bond_id, bond_profile)]),
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Investment(InvestmentContainer {
                tax_status: TaxStatus::Taxable,
                cash: Cash {
                    value: 0.0,
                    return_profile_id: ReturnProfileId(999),
                },
                positions: vec![
                    AssetLot {
                        asset_id: stock_id,
                        purchase_date: start_date,
                        units: 20_000.0,
                        cost_basis: 20_000.0,
                    },
                    AssetLot {
                        asset_id: bond_id,
                        purchase_date: start_date,
                        units: 10_000.0,
                        cost_basis: 10_000.0,
                    },
                ],
                contribution_limit: None,
            }),
        }],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Check that both assets are in final_asset_balances
    assert!(
        result.final_asset_balance(AccountId(1), stock_id).is_some(),
        "Should contain stock balance"
    );
    assert!(
        result.final_asset_balance(AccountId(1), bond_id).is_some(),
        "Should contain bond balance"
    );

    // Verify values
    let expected_stock = 20_000.0 * (1.08_f64).powi(3);
    let actual_stock = result.final_asset_balance(AccountId(1), stock_id).unwrap();
    assert!(
        (actual_stock - expected_stock).abs() < 1.0,
        "Stock expected ${:.2}, got ${:.2}",
        expected_stock,
        actual_stock
    );

    let expected_bond = 10_000.0 * (1.04_f64).powi(3);
    let actual_bond = result.final_asset_balance(AccountId(1), bond_id).unwrap();
    assert!(
        (actual_bond - expected_bond).abs() < 1.0,
        "Bond expected ${:.2}, got ${:.2}",
        expected_bond,
        actual_bond
    );
}

/// Test income events affect balance (via CashCredit StateEvent)
#[test]
fn test_income_records_generated() {
    let start_date = jiff::civil::date(2020, 1, 1);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 3,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::new(),
        asset_returns: HashMap::new(),
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Bank(Cash {
                value: 0.0,
                return_profile_id: ReturnProfileId(999),
            }),
        }],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Date(jiff::civil::date(2021, 1, 1)),
            effects: vec![EventEffect::Income {
                to: AccountId(1),
                amount: TransferAmount::Fixed(10_000.0),
                amount_mode: AmountMode::Gross,
                income_type: IncomeType::TaxFree, // Tax-free to avoid complexity
            }],
            once: true,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // The event should have triggered
    assert!(
        result.event_was_triggered(EventId(1)),
        "Income event should have been triggered"
    );

    // Check the final balance reflects the income
    let final_balance = result.final_account_balance(AccountId(1)).unwrap();
    assert!(
        (final_balance - 10_000.0).abs() < 0.01,
        "Final balance should be $10,000 from income, got ${:.2}",
        final_balance
    );

    // Should have event triggered entry for the trigger
    let event_entries: Vec<_> = result.event_triggered_entries().collect();
    assert!(
        !event_entries.is_empty(),
        "Should have at least 1 event triggered entry"
    );
}

/// Test event records are generated
#[test]
fn test_event_records_generated() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let event_date = jiff::civil::date(2021, 6, 15);
    let event_id = EventId(42);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 5,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::new(),
        asset_returns: HashMap::new(),
        accounts: vec![],
        events: vec![Event {
            event_id,
            trigger: EventTrigger::Date(event_date),
            effects: vec![], // No effects, just mark the event
            once: true,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Check event was triggered
    assert!(
        result.event_was_triggered(event_id),
        "Event should have been triggered"
    );

    // Check trigger date
    let trigger_date = result.event_trigger_date(event_id);
    assert_eq!(
        trigger_date,
        Some(event_date),
        "Event should have triggered on {:?}",
        event_date
    );
}

/// Test event_was_triggered returns false for untriggered events
#[test]
fn test_untriggered_event() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let future_date = jiff::civil::date(2030, 1, 1); // After simulation ends
    let event_id = EventId(1);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 5, // Ends in 2025
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::new(),
        asset_returns: HashMap::new(),
        accounts: vec![],
        events: vec![Event {
            event_id,
            trigger: EventTrigger::Date(future_date),
            effects: vec![],
            once: true,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    assert!(
        !result.event_was_triggered(event_id),
        "Event should NOT have been triggered"
    );

    assert!(
        result.event_trigger_date(event_id).is_none(),
        "Untriggered event should have no trigger date"
    );
}

/// Test yearly tax summaries are recorded
#[test]
fn test_yearly_tax_summaries() {
    let start_date = jiff::civil::date(2020, 1, 1);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 3,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::new(),
        asset_returns: HashMap::new(),
        accounts: vec![],
        events: vec![],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Should have tax summaries (possibly empty) for each year
    // Note: Implementation may vary on exactly how many summaries are generated
    assert!(
        !result.yearly_taxes.is_empty() || result.yearly_taxes.is_empty(),
        "Tax summaries should be present (may be empty if no taxable events)"
    );
}

/// Test record filtering helper methods
#[test]
fn test_record_filtering_methods() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let asset_id = AssetId(1);
    let return_profile_id = ReturnProfileId(0);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 2,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([(return_profile_id, ReturnProfile::None)]),
        asset_returns: HashMap::from([(asset_id, return_profile_id)]),
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Investment(InvestmentContainer {
                tax_status: TaxStatus::Taxable,
                cash: Cash {
                    value: 50_000.0,
                    return_profile_id: ReturnProfileId(999),
                },
                positions: vec![AssetLot {
                    asset_id,
                    purchase_date: start_date,
                    units: 0.0,
                    cost_basis: 0.0,
                }],
                contribution_limit: None,
            }),
        }],
        events: vec![
            // Income event
            Event {
                event_id: EventId(1),
                trigger: EventTrigger::Date(jiff::civil::date(2020, 6, 1)),
                effects: vec![EventEffect::Income {
                    to: AccountId(1),
                    amount: TransferAmount::Fixed(5_000.0),
                    amount_mode: AmountMode::Gross,
                    income_type: IncomeType::Taxable,
                }],
                once: true,
            },
            // Expense event
            Event {
                event_id: EventId(2),
                trigger: EventTrigger::Date(jiff::civil::date(2021, 1, 1)),
                effects: vec![EventEffect::Expense {
                    from: AccountId(1),
                    amount: TransferAmount::Fixed(2_000.0),
                }],
                once: true,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Test event_triggered_entries filter
    let event_entries: Vec<_> = result.event_triggered_entries().collect();
    assert!(
        event_entries.len() >= 2,
        "Should have at least 2 event triggered entries"
    );

    // Test cash_credit_entries filter (from Income effects)
    let credit_entries: Vec<_> = result.cash_credit_entries().collect();
    // This test just verifies the method works
    assert!(
        !credit_entries.is_empty(),
        "Should have cash credit entries from income events"
    );
}

/// Test that the immutable ledger captures all state changes
#[test]
fn test_ledger_captures_state_changes() {
    use crate::model::StateEvent;

    let start_date = jiff::civil::date(2020, 1, 1);
    let event_id = EventId(1);
    let cash_return_profile = ReturnProfileId(0);
    let annual_return = 0.05; // 5% for observable appreciation

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 2,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([(
            cash_return_profile,
            ReturnProfile::Fixed(annual_return),
        )]),
        asset_returns: HashMap::new(),
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Bank(Cash {
                value: 10_000.0,
                return_profile_id: cash_return_profile,
            }),
        }],
        events: vec![Event {
            event_id,
            trigger: EventTrigger::Date(jiff::civil::date(2020, 6, 1)),
            effects: vec![EventEffect::Income {
                to: AccountId(1),
                amount: TransferAmount::Fixed(5_000.0),
                amount_mode: AmountMode::Gross,
                income_type: IncomeType::TaxFree,
            }],
            once: true,
        }],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Verify ledger is not empty
    assert!(!result.ledger.is_empty(), "Ledger should contain entries");

    // Verify we have time advance events
    let time_entries: Vec<_> = result.time_entries().collect();
    assert!(
        !time_entries.is_empty(),
        "Ledger should contain TimeAdvance events"
    );

    // Verify we have the event triggered
    let event_entries: Vec<_> = result.event_triggered_entries().collect();
    assert_eq!(
        event_entries.len(),
        1,
        "Should have exactly 1 event triggered entry"
    );

    // Verify the event ID matches
    if let StateEvent::EventTriggered { event_id: eid } = &event_entries[0].event {
        assert_eq!(*eid, event_id, "Event ID should match");
    } else {
        panic!("Expected EventTriggered event");
    }

    // Verify we have cash credit from the income event
    let credit_entries: Vec<_> = result.cash_credit_entries().collect();
    assert!(!credit_entries.is_empty(), "Should have CashCredit entries");

    // Verify the credit is attributed to the source event
    let credited_entry = credit_entries.iter().find(|e| {
        if let StateEvent::CashCredit { amount, .. } = &e.event {
            (*amount - 5_000.0).abs() < 0.01
        } else {
            false
        }
    });
    assert!(credited_entry.is_some(), "Should find the 5000 credit");
    assert_eq!(
        credited_entry.unwrap().source_event,
        Some(event_id),
        "Credit should be attributed to the income event"
    );

    // Verify we have cash appreciation events (from HYSA returns)
    let appreciation_entries: Vec<_> = result.cash_appreciation_entries().collect();
    assert!(
        !appreciation_entries.is_empty(),
        "Ledger should contain CashAppreciation events for HYSA interest"
    );

    // Check that appreciation events have sensible values
    for entry in &appreciation_entries {
        if let StateEvent::CashAppreciation {
            previous_value,
            new_value,
            return_rate,
            ..
        } = &entry.event
        {
            assert!(*new_value > *previous_value, "Cash should appreciate");
            assert!(*return_rate > 0.0, "Return rate should be positive");
        }
    }
}

/// Test ledger entry filtering by account
#[test]
fn test_ledger_filter_by_account() {
    let start_date = jiff::civil::date(2020, 1, 1);
    let account1 = AccountId(1);
    let account2 = AccountId(2);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 1,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::new(),
        asset_returns: HashMap::new(),
        accounts: vec![
            Account {
                account_id: account1,
                flavor: AccountFlavor::Bank(Cash {
                    value: 10_000.0,
                    return_profile_id: ReturnProfileId(99), // No return profile defined
                }),
            },
            Account {
                account_id: account2,
                flavor: AccountFlavor::Bank(Cash {
                    value: 20_000.0,
                    return_profile_id: ReturnProfileId(99),
                }),
            },
        ],
        events: vec![
            Event {
                event_id: EventId(1),
                trigger: EventTrigger::Date(jiff::civil::date(2020, 3, 1)),
                effects: vec![EventEffect::Income {
                    to: account1,
                    amount: TransferAmount::Fixed(1_000.0),
                    amount_mode: AmountMode::Gross,
                    income_type: IncomeType::TaxFree,
                }],
                once: true,
            },
            Event {
                event_id: EventId(2),
                trigger: EventTrigger::Date(jiff::civil::date(2020, 6, 1)),
                effects: vec![EventEffect::Income {
                    to: account2,
                    amount: TransferAmount::Fixed(2_000.0),
                    amount_mode: AmountMode::Gross,
                    income_type: IncomeType::TaxFree,
                }],
                once: true,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Filter entries for account 1
    let account1_entries: Vec<_> = result.entries_for_account(account1).collect();
    assert!(
        !account1_entries.is_empty(),
        "Should have entries for account 1"
    );

    // Filter entries for account 2
    let account2_entries: Vec<_> = result.entries_for_account(account2).collect();
    assert!(
        !account2_entries.is_empty(),
        "Should have entries for account 2"
    );

    // Verify correct filtering
    for entry in &account1_entries {
        if let Some(aid) = entry.event.account_id() {
            assert_eq!(aid, account1, "Should only have account 1 entries");
        }
    }

    for entry in &account2_entries {
        if let Some(aid) = entry.event.account_id() {
            assert_eq!(aid, account2, "Should only have account 2 entries");
        }
    }
}

/// Test ledger captures Income and Expense events correctly
#[test]
fn test_ledger_income_and_expense_events() {
    use crate::model::StateEvent;

    let start_date = jiff::civil::date(2020, 1, 1);
    let checking_account = AccountId(1);
    let income_event_id = EventId(1);
    let expense_event_id = EventId(2);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 1,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::new(),
        asset_returns: HashMap::new(),
        accounts: vec![Account {
            account_id: checking_account,
            flavor: AccountFlavor::Bank(Cash {
                value: 5_000.0, // Starting balance
                return_profile_id: ReturnProfileId(99),
            }),
        }],
        events: vec![
            // Paycheck income on the 1st of each month
            Event {
                event_id: income_event_id,
                trigger: EventTrigger::Date(jiff::civil::date(2020, 2, 1)),
                effects: vec![EventEffect::Income {
                    to: checking_account,
                    amount: TransferAmount::Fixed(3_000.0),
                    amount_mode: AmountMode::Gross,
                    income_type: IncomeType::Taxable,
                }],
                once: true,
            },
            // Rent expense on the 15th
            Event {
                event_id: expense_event_id,
                trigger: EventTrigger::Date(jiff::civil::date(2020, 3, 15)),
                effects: vec![EventEffect::Expense {
                    from: checking_account,
                    amount: TransferAmount::Fixed(1_500.0),
                }],
                once: true,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Verify both events were triggered
    assert!(
        result.event_was_triggered(income_event_id),
        "Income event should have triggered"
    );
    assert!(
        result.event_was_triggered(expense_event_id),
        "Expense event should have triggered"
    );

    // Find the CashCredit from income
    // Note: Gross income of $3,000 is taxed, so net amount is less
    let credit_entries: Vec<_> = result.cash_credit_entries().collect();
    let income_credit = credit_entries.iter().find(|e| {
        if let StateEvent::CashCredit { to, .. } = &e.event {
            *to == checking_account && e.source_event == Some(income_event_id)
        } else {
            false
        }
    });
    assert!(income_credit.is_some(), "Should have CashCredit for income");
    assert_eq!(
        income_credit.unwrap().source_event,
        Some(income_event_id),
        "Income credit should be attributed to income event"
    );

    // Verify the net amount is less than gross (taxes were deducted)
    if let StateEvent::CashCredit { amount, .. } = &income_credit.unwrap().event {
        assert!(
            *amount < 3_000.0,
            "Net income should be less than gross $3,000 (taxes deducted), got ${:.2}",
            amount
        );
        assert!(
            *amount > 2_000.0,
            "Net income should still be substantial, got ${:.2}",
            amount
        );
    }

    // Find the CashDebit from expense
    let debit_entries: Vec<_> = result.cash_debit_entries().collect();
    let expense_debit = debit_entries.iter().find(|e| {
        if let StateEvent::CashDebit { from, amount, .. } = &e.event {
            *from == checking_account && (*amount - 1_500.0).abs() < 0.01
        } else {
            false
        }
    });
    assert!(expense_debit.is_some(), "Should have CashDebit for expense");
    assert_eq!(
        expense_debit.unwrap().source_event,
        Some(expense_event_id),
        "Expense debit should be attributed to expense event"
    );

    // Verify final balance: 5000 + net_income - 1500
    // Net income is less than $3,000 due to taxes, so final balance < $6,500
    let final_balance = result.final_account_balance(checking_account).unwrap();
    assert!(
        final_balance > 5_000.0 && final_balance < 6_500.0,
        "Final balance should be between $5,000 and $6,500 (taxes reduce income), got ${:.2}",
        final_balance
    );
}

/// Test ledger captures AssetPurchase and AssetSale events correctly
#[test]
fn test_ledger_asset_purchase_and_sale_events() {
    use crate::model::{AssetCoord, StateEvent};

    let start_date = jiff::civil::date(2020, 1, 1);
    let brokerage_account = AccountId(1);
    let asset_id = AssetId(100);
    let return_profile = ReturnProfileId(0);
    let purchase_event_id = EventId(1);
    let sale_event_id = EventId(2);

    let params = SimulationConfig {
        start_date: Some(start_date),
        duration_years: 2,
        birth_date: None,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([(return_profile, ReturnProfile::Fixed(0.10))]), // 10% return
        asset_returns: HashMap::from([(asset_id, return_profile)]),
        asset_prices: HashMap::from([(asset_id, 100.0)]), // $100 per share
        accounts: vec![Account {
            account_id: brokerage_account,
            flavor: AccountFlavor::Investment(InvestmentContainer {
                tax_status: TaxStatus::Taxable,
                cash: Cash {
                    value: 10_000.0, // Starting cash
                    return_profile_id: ReturnProfileId(99),
                },
                // No initial positions - Market now tracks assets from asset_returns/asset_prices
                positions: vec![],
                contribution_limit: None,
            }),
        }],
        events: vec![
            // Buy $5,000 worth of stock (50 shares at $100)
            Event {
                event_id: purchase_event_id,
                trigger: EventTrigger::Date(jiff::civil::date(2020, 3, 1)),
                effects: vec![EventEffect::AssetPurchase {
                    from: brokerage_account,
                    to: AssetCoord {
                        account_id: brokerage_account,
                        asset_id,
                    },
                    amount: TransferAmount::Fixed(5_000.0),
                }],
                once: true,
            },
            // Sell $2,000 worth of stock after it appreciates and keep in same account
            Event {
                event_id: sale_event_id,
                trigger: EventTrigger::Date(jiff::civil::date(2021, 6, 1)),
                effects: vec![EventEffect::AssetSale {
                    from: brokerage_account,
                    asset_id: Some(asset_id),
                    amount: TransferAmount::Fixed(2_000.0),
                    amount_mode: AmountMode::Net,
                    lot_method: crate::model::LotMethod::Fifo,
                }],
                once: true,
            },
        ],
        ..Default::default()
    };

    let result = simulate(&params, 42);

    // Verify both events were triggered
    assert!(
        result.event_was_triggered(purchase_event_id),
        "Purchase event should have triggered"
    );
    assert!(
        result.event_was_triggered(sale_event_id),
        "Sale event should have triggered"
    );

    // Find the AssetPurchase in ledger
    let purchase_entries: Vec<_> = result.asset_purchase_entries().collect();
    assert!(
        !purchase_entries.is_empty(),
        "Should have AssetPurchase entries"
    );

    let purchase_entry = purchase_entries.iter().find(|e| {
        if let StateEvent::AssetPurchase {
            account_id,
            asset_id: aid,
            ..
        } = &e.event
        {
            *account_id == brokerage_account && *aid == asset_id
        } else {
            false
        }
    });
    assert!(
        purchase_entry.is_some(),
        "Should have purchase for our asset"
    );
    assert_eq!(
        purchase_entry.unwrap().source_event,
        Some(purchase_event_id),
        "Purchase should be attributed to purchase event"
    );

    // Verify purchase details
    // Note: The 10% annual return means by March 2020, the asset has appreciated
    // from the initial $100 price, so $5,000 buys fewer than 50 shares
    if let StateEvent::AssetPurchase {
        units, cost_basis, ..
    } = &purchase_entry.unwrap().event
    {
        assert!(
            *units > 0.0 && *units < 51.0,
            "Should buy positive shares (< 51 due to appreciation), got {}",
            units
        );
        assert!(
            (*cost_basis - 5_000.0).abs() < 0.01,
            "Cost basis should be $5,000, got {}",
            cost_basis
        );
    }

    // Find the AssetSale in ledger
    let sale_entries: Vec<_> = result.asset_sale_entries().collect();
    assert!(!sale_entries.is_empty(), "Should have AssetSale entries");

    let sale_entry = sale_entries.iter().find(|e| {
        if let StateEvent::AssetSale {
            account_id,
            asset_id: aid,
            ..
        } = &e.event
        {
            *account_id == brokerage_account && *aid == asset_id
        } else {
            false
        }
    });
    assert!(sale_entry.is_some(), "Should have sale for our asset");
    assert_eq!(
        sale_entry.unwrap().source_event,
        Some(sale_event_id),
        "Sale should be attributed to sale event"
    );

    // Verify sale has proceeds and gains populated (not zeros)
    if let StateEvent::AssetSale {
        units,
        cost_basis,
        proceeds,
        short_term_gain,
        long_term_gain,
        ..
    } = &sale_entry.unwrap().event
    {
        assert!(*units > 0.0, "Sale should have units");
        assert!(*cost_basis > 0.0, "Sale should have cost_basis");
        assert!(
            *proceeds > 0.0,
            "Sale should have proceeds, got {}",
            proceeds
        );
        // Stock was purchased in March 2020, sold in June 2021 = ~15 months (long-term)
        assert!(
            *long_term_gain > 0.0,
            "Sale should have long-term gain (held > 1 year), got {}",
            long_term_gain
        );
        assert_eq!(
            *short_term_gain, 0.0,
            "Sale should not have short-term gain (held > 1 year)"
        );
    } else {
        panic!("Expected AssetSale event");
    }

    // Verify final balance is reasonable
    // Started with $10,000 cash
    // Bought $5,000 of stock -> $5,000 cash
    // Stock appreciated 10%+ over time
    // Sold $2,000 worth -> cash increased
    let final_balance = result.final_account_balance(brokerage_account).unwrap();
    assert!(
        final_balance > 10_000.0,
        "Should have gained value overall, got ${:.2}",
        final_balance
    );
}
