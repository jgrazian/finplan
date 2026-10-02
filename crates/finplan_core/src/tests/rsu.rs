//! RSU (Restricted Stock Unit) vesting tests
//!
//! Tests for the RsuVesting event effect including:
//! - Basic vesting with correct cost basis
//! - Income tax on FMV at vesting
//! - Sell-to-cover mechanics
//! - Quarterly vesting schedules
//! - Capital gains after holding vested shares

use crate::config::{AccountBuilder, AssetBuilder, EventBuilder, SimulationBuilder};
use crate::simulation::simulate;

/// Helper to find account balance by name after simulation
fn account_balance(
    result: &crate::model::SimulationResult,
    metadata: &crate::config::SimulationMetadata,
    name: &str,
) -> f64 {
    let account_id = metadata.account_id(name).expect("account not found");
    result
        .wealth_snapshots
        .last()
        .expect("no snapshots")
        .accounts
        .iter()
        .find(|snap| snap.account_id == account_id)
        .map(|snap| snap.total_value())
        .unwrap_or(0.0)
}

/// Basic RSU vesting: shares appear as AssetLots with cost basis = FMV at vesting
#[test]
fn test_basic_rsu_vesting() {
    // Stock at $100/share, 0% return so price stays constant
    // Vest 10 shares = $1,000 FMV
    let (config, metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(1)
        .inflation(0.0)
        .asset(AssetBuilder::new("GOOG").price(100.0).fixed_return(0.0))
        .account(AccountBuilder::taxable_brokerage("Brokerage").cash(0.0))
        .event(
            EventBuilder::rsu_vesting("RSU Vest")
                .to_account("Brokerage")
                .asset_in("Brokerage", "GOOG")
                .units(10.0)
                .on_date(jiff::civil::date(2025, 3, 15))
                .once(),
        )
        .build();

    let result = simulate(&config, 42).unwrap();

    // After vesting, brokerage should have shares worth ~$1,000
    let brokerage_balance = account_balance(&result, &metadata, "Brokerage");
    assert!(
        brokerage_balance > 0.0,
        "Brokerage should have a positive balance after RSU vesting, got {}",
        brokerage_balance
    );

    // Check that income tax was recorded in yearly tax summaries
    let total_tax: f64 = result.yearly_taxes.iter().map(|t| t.total_tax).sum();
    assert!(
        total_tax > 0.0,
        "Income tax should be recorded on RSU vesting FMV"
    );
}

/// RSU income tax: verify ordinary income tax is calculated on full FMV
#[test]
fn test_rsu_income_tax() {
    // Stock at $200/share, vest 50 shares = $10,000 FMV
    // With default tax config: 10% on first $11,600 → $1,000 federal on $10,000
    // Plus 5% state = $500
    // Total tax = $1,500 on $10,000 gross
    let (config, _metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(1)
        .inflation(0.0)
        .asset(AssetBuilder::new("GOOG").price(200.0).fixed_return(0.0))
        .account(AccountBuilder::taxable_brokerage("Brokerage").cash(0.0))
        .event(
            EventBuilder::rsu_vesting("RSU Vest")
                .to_account("Brokerage")
                .asset_in("Brokerage", "GOOG")
                .units(50.0)
                .on_date(jiff::civil::date(2025, 3, 15))
                .once(),
        )
        .build();

    let result = simulate(&config, 42).unwrap();

    let total_federal: f64 = result.yearly_taxes.iter().map(|t| t.federal_tax).sum();
    let total_state: f64 = result.yearly_taxes.iter().map(|t| t.state_tax).sum();

    // $10,000 FMV → federal tax at 10% marginal = $1,000
    assert!(
        total_federal > 900.0 && total_federal < 1_100.0,
        "Federal income tax on $10k FMV should be ~$1,000, got {}",
        total_federal
    );

    // $10,000 FMV → state tax at 5% = $500
    assert!(
        (total_state - 500.0).abs() < 1.0,
        "State income tax on $10k FMV should be $500, got {}",
        total_state
    );
}

/// Sell-to-cover: verify correct number of shares sold, zero capital gain
#[test]
fn test_rsu_sell_to_cover() {
    // Stock at $100/share, vest 100 shares = $10,000 FMV
    // With sell-to-cover: taxes paid by selling shares at cost basis = FMV
    // So zero capital gain on the sold portion
    let (config, metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(1)
        .inflation(0.0)
        .asset(AssetBuilder::new("GOOG").price(100.0).fixed_return(0.0))
        .account(AccountBuilder::taxable_brokerage("Brokerage").cash(0.0))
        .event(
            EventBuilder::rsu_vesting("RSU Vest")
                .to_account("Brokerage")
                .asset_in("Brokerage", "GOOG")
                .units(100.0)
                .sell_to_cover()
                .on_date(jiff::civil::date(2025, 3, 15))
                .once(),
        )
        .build();

    let result = simulate(&config, 42).unwrap();
    let brokerage_balance = account_balance(&result, &metadata, "Brokerage");

    // With sell-to-cover, all tax is paid by selling shares
    // No out-of-pocket tax cost, but fewer shares remain
    // $10,000 FMV, ~$1,500 in taxes → ~15 shares sold → ~85 shares remain = ~$8,500
    assert!(
        brokerage_balance > 8_000.0 && brokerage_balance < 9_000.0,
        "Brokerage balance after sell-to-cover should be ~$8,500, got {}",
        brokerage_balance
    );

    // Verify no capital gains were incurred (sold at cost basis = FMV)
    let total_cap_gains: f64 = result.yearly_taxes.iter().map(|t| t.capital_gains).sum();
    assert!(
        total_cap_gains.abs() < 1.0,
        "Sell-to-cover should have zero capital gains, got {}",
        total_cap_gains
    );
}

/// Quarterly vesting schedule: verify 4 vests over a year
#[test]
fn test_rsu_quarterly_vesting() {
    // Vest 25 shares quarterly for 4 quarters = 100 shares total
    let (config, metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(2)
        .inflation(0.0)
        .asset(AssetBuilder::new("GOOG").price(100.0).fixed_return(0.0))
        .account(AccountBuilder::taxable_brokerage("Brokerage").cash(0.0))
        .event(
            EventBuilder::rsu_vesting("RSU Vest")
                .to_account("Brokerage")
                .asset_in("Brokerage", "GOOG")
                .units(25.0)
                .quarterly()
                .starting_on(jiff::civil::date(2025, 3, 15))
                .max_occurrences(4),
        )
        .build();

    let result = simulate(&config, 42).unwrap();
    let brokerage_balance = account_balance(&result, &metadata, "Brokerage");

    // 4 vests × 25 shares × $100 = $10,000 in shares deposited
    // With no sell-to-cover, all shares remain
    // The balance should reflect the full $10,000 in stock value
    assert!(
        brokerage_balance > 9_000.0,
        "Brokerage should have ~$10,000 in shares from 4 quarterly vests, got {}",
        brokerage_balance
    );
}

/// RSU sale after vesting: capital gains calculated relative to vesting FMV
#[test]
fn test_rsu_sale_after_vesting() {
    // Vest shares at $100, then stock appreciates (10% annual return)
    // Sell after 1+ years → long-term capital gain on the appreciation
    let (config, metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(2)
        .birth_date(1980, 1, 1)
        .inflation(0.0)
        .asset(AssetBuilder::new("GOOG").price(100.0).fixed_return(0.10))
        .account(AccountBuilder::taxable_brokerage("Brokerage").cash(0.0))
        .account(AccountBuilder::bank_account("Checking").cash(0.0))
        // Vest 100 shares at $100 on 2025-03-15
        .event(
            EventBuilder::rsu_vesting("RSU Vest")
                .to_account("Brokerage")
                .asset_in("Brokerage", "GOOG")
                .units(100.0)
                .sell_to_cover()
                .on_date(jiff::civil::date(2025, 3, 15))
                .once(),
        )
        // Sell all shares ~1.5 years after vesting
        .event(
            EventBuilder::withdrawal("Sell GOOG")
                .to_account("Checking")
                .amount(50_000.0) // Large amount to sell everything
                .gross()
                .from_single_account("Brokerage")
                .on_date(jiff::civil::date(2026, 9, 15))
                .once(),
        )
        .build();

    let result = simulate(&config, 42).unwrap();

    // After selling, there should be capital gains recorded
    // The gain is on the appreciation above the vesting FMV ($100 cost basis per share)
    let total_cap_gains: f64 = result.yearly_taxes.iter().map(|t| t.capital_gains).sum();

    // The shares appreciated from $100 over 1.5 years at 10% return
    // So there should be some capital gains (held > 1 year from vest date)
    assert!(
        total_cap_gains > 0.0,
        "Should have capital gains from selling appreciated RSU shares, got {}",
        total_cap_gains
    );

    // Checking account should have received sale proceeds
    let checking_balance = account_balance(&result, &metadata, "Checking");
    assert!(
        checking_balance > 0.0,
        "Checking should have received sale proceeds, got {}",
        checking_balance
    );
}

/// Test the rsu_grant convenience method on SimulationBuilder
#[test]
fn test_rsu_grant_convenience() {
    let (config, metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(2)
        .inflation(0.0)
        .asset(AssetBuilder::new("GOOG").price(150.0).fixed_return(0.0))
        .account(AccountBuilder::taxable_brokerage("Brokerage").cash(0.0))
        .rsu_grant(
            "GOOG RSU",
            "Brokerage",
            "GOOG",
            25.0,
            4,
            jiff::civil::date(2025, 3, 15),
        )
        .build();

    assert_eq!(config.events.len(), 1);
    assert!(metadata.event_id("GOOG RSU").is_some());

    // Verify the event has the RSU vesting effect
    let event = &config.events[0];
    assert_eq!(event.effects.len(), 1);
    assert!(matches!(
        event.effects[0],
        crate::model::EventEffect::RsuVesting {
            sell_to_cover: true,
            ..
        }
    ));

    // Run simulation to verify it works end-to-end
    let result = simulate(&config, 42).unwrap();
    let brokerage_balance = account_balance(&result, &metadata, "Brokerage");
    assert!(
        brokerage_balance > 0.0,
        "Brokerage should have shares from RSU vesting"
    );
}

/// Federal bracket thresholds are indexed to inflation each tax year, so the
/// same nominal income is taxed less once prices have risen.
#[test]
fn test_federal_brackets_index_to_inflation() {
    // $50,000 FMV vests in 2025 and again in 2027, at 10% inflation.
    let vest = |name: &str, year: i16| {
        EventBuilder::rsu_vesting(name)
            .to_account("Brokerage")
            .asset_in("Brokerage", "GOOG")
            .units(250.0)
            .on_date(jiff::civil::date(year, 3, 15))
            .once()
    };
    let (config, _metadata) = SimulationBuilder::new()
        .start(2025, 1, 1)
        .years(3)
        .inflation(0.10)
        .asset(AssetBuilder::new("GOOG").price(200.0).fixed_return(0.0))
        .account(AccountBuilder::taxable_brokerage("Brokerage").cash(0.0))
        .event(vest("Vest 2025", 2025))
        .event(vest("Vest 2027", 2027))
        .build();

    let result = simulate(&config, 42).unwrap();
    let federal = |year: i16| {
        result
            .yearly_taxes
            .iter()
            .find(|t| t.year == year)
            .map(|t| t.federal_tax)
            .unwrap()
    };

    // 2025, configured brackets: 10% to $11,600, 12% to $47,150, then 22%.
    let expected_2025 = 11_600.0 * 0.10 + 35_550.0 * 0.12 + 2_850.0 * 0.22;
    assert!((federal(2025) - expected_2025).abs() < 0.01);

    // 2027, thresholds scaled by 1.1^2: the whole $50,000 stays under 22%.
    let (t1, t2) = (11_600.0 * 1.21, 47_150.0 * 1.21);
    let expected_2027 = t1 * 0.10 + (50_000.0 - t1) * 0.12;
    assert!(t2 > 50_000.0);
    assert!(
        (federal(2027) - expected_2027).abs() < 0.01,
        "2027 federal tax {} != {expected_2027}",
        federal(2027)
    );
}

/// The standard deduction shields each year's first income, is indexed with
/// the brackets, and grows by the 65+ extra from the year the filer turns 65.
#[test]
fn test_standard_deduction_and_age_65_extra() {
    use crate::model::TaxConfig;

    let federal_by_year = |birth: jiff::civil::Date| {
        let vest = |name: &str, year: i16| {
            EventBuilder::rsu_vesting(name)
                .to_account("Brokerage")
                .asset_in("Brokerage", "GOOG")
                .units(250.0)
                .on_date(jiff::civil::date(year, 3, 15))
                .once()
        };
        let (config, _metadata) = SimulationBuilder::new()
            .start(2025, 1, 1)
            .years(3)
            .inflation(0.10)
            .birth_date_obj(birth)
            .tax_config(TaxConfig {
                standard_deduction: 15_000.0,
                age_65_extra_deduction: 2_000.0,
                ..TaxConfig::default()
            })
            .asset(AssetBuilder::new("GOOG").price(200.0).fixed_return(0.0))
            .account(AccountBuilder::taxable_brokerage("Brokerage").cash(0.0))
            .event(vest("Vest 2025", 2025))
            .event(vest("Vest 2026", 2026))
            .build();
        let result = simulate(&config, 42).unwrap();
        result
            .yearly_taxes
            .iter()
            .map(|t| (t.year, t.federal_tax))
            .collect::<Vec<_>>()
    };
    // $50,000 of income against brackets at 10% from $11,600 and 12% from
    // $47,150, all moved up by the deduction and scaled by inflation.
    let expected = |deduction: f64, factor: f64| {
        let zero_top = deduction * factor;
        let ten_top = (deduction + 11_600.0) * factor;
        (ten_top - zero_top) * 0.10 + (50_000.0 - ten_top) * 0.12
    };
    let close = |a: f64, b: f64| (a - b).abs() < 0.01;

    // Turns 65 in mid-2026: the extra starts with the 2026 tax year.
    let taxes = federal_by_year(jiff::civil::date(1961, 6, 15));
    assert!(close(taxes[0].1, expected(15_000.0, 1.0)), "{taxes:?}");
    assert!(close(taxes[1].1, expected(17_000.0, 1.1)), "{taxes:?}");

    // Born January 1st, 1961: counted 65 on December 31st, 2025.
    let taxes = federal_by_year(jiff::civil::date(1961, 1, 1));
    assert!(close(taxes[0].1, expected(17_000.0, 1.0)), "{taxes:?}");
    let taxes = federal_by_year(jiff::civil::date(1961, 1, 2));
    assert!(close(taxes[0].1, expected(15_000.0, 1.0)), "{taxes:?}");
}
