//! Tests for the Builder DSL
//!
//! These tests demonstrate and verify the fluent builder API for creating simulations.

use crate::config::{AccountBuilder, AssetBuilder, EventBuilder, SimulationBuilder};
use crate::model::{AccountFlavor, TaxStatus};
use crate::simulation::simulate;

/// Test basic SimulationBuilder usage
#[test]
fn test_simulation_builder_basic() {
    let (config, metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(5)
        .birth_date(1980, 6, 15)
        .build();

    assert_eq!(config.start_date, Some(jiff::civil::date(2025, 1, 1)));
    assert_eq!(config.duration_years, 5);
    assert_eq!(config.birth_date, Some(jiff::civil::date(1980, 6, 15)));

    // Metadata should be empty since we didn't add any entities
    assert!(metadata.accounts.is_empty());
    assert!(metadata.assets.is_empty());
    assert!(metadata.events.is_empty());
}

/// Test AccountBuilder preset types
#[test]
fn test_account_builder_presets() {
    let (config, metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(30)
        .account(AccountBuilder::bank_account("Checking").cash(10_000.0))
        .account(AccountBuilder::taxable_brokerage("Brokerage").cash(50_000.0))
        .account(AccountBuilder::traditional_401k("Work 401k").cash(200_000.0))
        .account(AccountBuilder::roth_ira("Roth IRA").cash(100_000.0))
        .account(AccountBuilder::hsa("HSA").cash(20_000.0))
        .build();

    assert_eq!(config.accounts.len(), 5);

    // Verify metadata has all account names
    assert!(metadata.account_id("Checking").is_some());
    assert!(metadata.account_id("Brokerage").is_some());
    assert!(metadata.account_id("Work 401k").is_some());
    assert!(metadata.account_id("Roth IRA").is_some());
    assert!(metadata.account_id("HSA").is_some());

    // Verify account types
    let checking_id = metadata.account_id("Checking").unwrap();
    let checking = config
        .accounts
        .iter()
        .find(|a| a.account_id == checking_id)
        .unwrap();
    assert!(matches!(checking.flavor, AccountFlavor::Bank(_)));

    let brokerage_id = metadata.account_id("Brokerage").unwrap();
    let brokerage = config
        .accounts
        .iter()
        .find(|a| a.account_id == brokerage_id)
        .unwrap();
    if let AccountFlavor::Investment(inv) = &brokerage.flavor {
        assert!(matches!(inv.tax_status, TaxStatus::Taxable));
    } else {
        panic!("Expected Investment flavor for brokerage");
    }

    let trad_401k_id = metadata.account_id("Work 401k").unwrap();
    let trad_401k = config
        .accounts
        .iter()
        .find(|a| a.account_id == trad_401k_id)
        .unwrap();
    if let AccountFlavor::Investment(inv) = &trad_401k.flavor {
        assert!(matches!(inv.tax_status, TaxStatus::TaxDeferred));
    } else {
        panic!("Expected Investment flavor for 401k");
    }

    let roth_id = metadata.account_id("Roth IRA").unwrap();
    let roth = config
        .accounts
        .iter()
        .find(|a| a.account_id == roth_id)
        .unwrap();
    if let AccountFlavor::Investment(inv) = &roth.flavor {
        assert!(matches!(inv.tax_status, TaxStatus::TaxFree));
    } else {
        panic!("Expected Investment flavor for Roth IRA");
    }
}

/// Test AssetBuilder with return profiles
#[test]
fn test_asset_builder_with_profiles() {
    let (config, metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(10)
        .asset(AssetBuilder::us_total_market("VTSAX").price(100.0))
        .asset(AssetBuilder::total_bond("BND").price(50.0))
        .asset(AssetBuilder::new("AAPL").price(150.0).fixed_return(0.12))
        .build();

    // Verify assets are registered
    assert!(metadata.asset_id("VTSAX").is_some());
    assert!(metadata.asset_id("BND").is_some());
    assert!(metadata.asset_id("AAPL").is_some());

    // Verify asset prices
    let vtsax_id = metadata.asset_id("VTSAX").unwrap();
    assert_eq!(config.asset_prices.get(&vtsax_id), Some(&100.0));

    let bnd_id = metadata.asset_id("BND").unwrap();
    assert_eq!(config.asset_prices.get(&bnd_id), Some(&50.0));

    let aapl_id = metadata.asset_id("AAPL").unwrap();
    assert_eq!(config.asset_prices.get(&aapl_id), Some(&150.0));

    // Verify return profiles are created
    assert!(!config.return_profiles.is_empty());
}

/// Test adding positions to accounts
#[test]
fn test_positions() {
    let (config, metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(10)
        .asset(AssetBuilder::us_total_market("VTSAX").price(100.0))
        .asset(AssetBuilder::total_bond("BND").price(50.0))
        .account(AccountBuilder::taxable_brokerage("Brokerage").cash(5_000.0))
        .account(AccountBuilder::traditional_401k("401k").cash(10_000.0))
        .position("Brokerage", "VTSAX", 500.0, 45_000.0)
        .position("Brokerage", "BND", 200.0, 9_000.0)
        .position("401k", "VTSAX", 1000.0, 90_000.0)
        .build();

    let brokerage_id = metadata.account_id("Brokerage").unwrap();
    let brokerage = config
        .accounts
        .iter()
        .find(|a| a.account_id == brokerage_id)
        .unwrap();

    if let AccountFlavor::Investment(inv) = &brokerage.flavor {
        assert_eq!(inv.positions.len(), 2);
        assert_eq!(inv.cash.value, 5_000.0);
    } else {
        panic!("Expected Investment flavor");
    }

    let _401k_id = metadata.account_id("401k").unwrap();
    let _401k = config
        .accounts
        .iter()
        .find(|a| a.account_id == _401k_id)
        .unwrap();

    if let AccountFlavor::Investment(inv) = &_401k.flavor {
        assert_eq!(inv.positions.len(), 1);
        assert_eq!(inv.positions[0].units, 1000.0);
    } else {
        panic!("Expected Investment flavor");
    }
}

/// Test EventBuilder for income
#[test]
fn test_income_event_builder() {
    let (config, metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(5)
        .birth_date(1980, 1, 1)
        .account(AccountBuilder::bank_account("Checking").cash(1_000.0))
        .event(
            EventBuilder::income("Salary")
                .to_account("Checking")
                .amount(8_000.0)
                .gross()
                .monthly()
                .until_age(65),
        )
        .build();

    assert_eq!(config.events.len(), 1);
    assert!(metadata.event_id("Salary").is_some());

    let salary_id = metadata.event_id("Salary").unwrap();
    let salary_event = config
        .events
        .iter()
        .find(|e| e.event_id == salary_id)
        .unwrap();
    assert_eq!(salary_event.effects.len(), 1);
}

/// Test EventBuilder for expense
#[test]
fn test_expense_event_builder() {
    let (config, metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(5)
        .account(AccountBuilder::bank_account("Checking").cash(50_000.0))
        .event(
            EventBuilder::expense("Rent")
                .from_account("Checking")
                .amount(2_000.0)
                .monthly(),
        )
        .event(
            EventBuilder::expense("Utilities")
                .from_account("Checking")
                .amount(200.0)
                .monthly(),
        )
        .build();

    assert_eq!(config.events.len(), 2);
    assert!(metadata.event_id("Rent").is_some());
    assert!(metadata.event_id("Utilities").is_some());
}

/// Test full simulation setup with builder
#[test]
fn test_full_simulation_with_builder() {
    let (config, _metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(2)
        .inflation(0.03)
        .asset(AssetBuilder::us_total_market("VTSAX").price(100.0))
        .account(AccountBuilder::bank_account("Checking").cash(10_000.0))
        .account(AccountBuilder::taxable_brokerage("Brokerage").cash(5_000.0))
        .position("Brokerage", "VTSAX", 100.0, 10_000.0)
        .event(
            EventBuilder::income("Salary")
                .to_account("Checking")
                .amount(5_000.0)
                .gross()
                .monthly(),
        )
        .event(
            EventBuilder::expense("Living")
                .from_account("Checking")
                .amount(3_000.0)
                .monthly(),
        )
        .build();

    // Run simulation
    let result = simulate(&config, 42);

    // Basic validation
    assert!(!result.wealth_snapshots.is_empty());
    assert_eq!(
        result.wealth_snapshots.first().map(|snap| snap.date),
        Some(jiff::civil::date(2025, 1, 1))
    );
}

/// Test quick builder methods
#[test]
fn test_quick_builder_methods() {
    let (config, metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(5)
        .bank("Checking", 10_000.0)
        .brokerage("Brokerage", 50_000.0)
        .traditional_401k("401k", 200_000.0)
        .roth_ira("Roth", 100_000.0)
        .asset_fixed("VTSAX", 100.0, 0.10)
        .monthly_income("Salary", "Checking", 8_000.0)
        .monthly_expense("Rent", "Checking", 2_000.0)
        .build();

    assert_eq!(config.accounts.len(), 4);
    assert_eq!(config.events.len(), 2);

    assert!(metadata.account_id("Checking").is_some());
    assert!(metadata.account_id("Brokerage").is_some());
    assert!(metadata.account_id("401k").is_some());
    assert!(metadata.account_id("Roth").is_some());
    assert!(metadata.asset_id("VTSAX").is_some());
    assert!(metadata.event_id("Salary").is_some());
    assert!(metadata.event_id("Rent").is_some());
}

/// Test metadata bidirectional lookups
#[test]
fn test_metadata_lookups() {
    let (config, metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(5)
        .account(AccountBuilder::bank_account("My Checking").cash(1_000.0))
        .asset(AssetBuilder::new("VTSAX").price(100.0).fixed_return(0.10))
        .event(
            EventBuilder::income("Monthly Salary")
                .to_account("My Checking")
                .amount(5_000.0)
                .monthly(),
        )
        .build();

    // Forward lookup: name -> id
    let checking_id = metadata.account_id("My Checking").unwrap();
    let vtsax_id = metadata.asset_id("VTSAX").unwrap();
    let salary_id = metadata.event_id("Monthly Salary").unwrap();

    // Verify IDs exist in config
    assert!(config.accounts.iter().any(|a| a.account_id == checking_id));
    assert!(config.asset_prices.contains_key(&vtsax_id));
    assert!(config.events.iter().any(|e| e.event_id == salary_id));

    // Reverse lookup: id -> name
    assert_eq!(metadata.account_name(checking_id), Some("My Checking"));
    assert_eq!(metadata.asset_name(vtsax_id), Some("VTSAX"));
    assert_eq!(metadata.event_name(salary_id), Some("Monthly Salary"));
}

/// Test liability account (mortgage)
#[test]
fn test_liability_account() {
    let (config, metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(30)
        .account(AccountBuilder::mortgage("Home Mortgage", 300_000.0, 0.065))
        .account(AccountBuilder::student_loan(
            "Student Loans",
            50_000.0,
            0.05,
        ))
        .build();

    assert_eq!(config.accounts.len(), 2);

    let mortgage_id = metadata.account_id("Home Mortgage").unwrap();
    let mortgage = config
        .accounts
        .iter()
        .find(|a| a.account_id == mortgage_id)
        .unwrap();

    if let AccountFlavor::Liability(loan) = &mortgage.flavor {
        assert_eq!(loan.principal, 300_000.0);
        assert_eq!(loan.interest_rate, 0.065);
    } else {
        panic!("Expected Liability flavor for mortgage");
    }
}
