//! Starter data for a new account.
//!
//! A scenario cannot reference a return profile that does not exist, so every
//! user is given a small library of market assumptions and a default tax table
//! at registration. These are ordinary rows: the user can edit or delete them.

use crate::api::profiles::AssetClass;
use crate::db::Db;
use crate::error::ApiResult;

/// (name, description, distribution kind, mean/rate, std_dev, asset class)
///
/// The figures are the long-run historical constants the engine ships with, in
/// `finplan_core::model::market`.
///
/// The class is what a ticker's asset class is matched against, so it is what
/// lets the web client map `VTI` onto the first of these without reading its
/// name. Two carry none: "Savings Account" describes a bank balance rather than
/// a holding, and "No Growth" is the absence of an assumption — neither should
/// ever be what a ticker resolves to.
type SeedProfile = (
    &'static str,
    &'static str,
    &'static str,
    f64,
    f64,
    Option<AssetClass>,
);

const RETURN_PROFILES: &[SeedProfile] = &[
    (
        "US Total Market",
        "S&P 500, 1928-2024: 9.9% mean, 19.6% sd",
        "Normal",
        0.0990829,
        0.1962,
        Some(AssetClass::UsEquity),
    ),
    (
        "US Small Cap",
        "US small cap, 1928-2024: 11.2% mean",
        "Normal",
        0.112028,
        0.2988,
        Some(AssetClass::UsSmallCap),
    ),
    (
        "US Aggregate Bonds",
        "US aggregate bonds: 3.0% mean",
        "Normal",
        0.0301011,
        0.0574,
        Some(AssetClass::Bonds),
    ),
    (
        "International Developed",
        "Developed ex-US equities: 6.0% mean",
        "Normal",
        0.0602527,
        0.2251,
        Some(AssetClass::IntlEquity),
    ),
    (
        "REITs",
        "US real estate investment trusts: 6.4% mean",
        "Normal",
        0.0642145,
        0.1959,
        Some(AssetClass::Reit),
    ),
    (
        "Cash / T-Bills",
        "US Treasury bills: 3.4% mean",
        "Normal",
        0.0337398,
        0.0308,
        Some(AssetClass::Cash),
    ),
    (
        "Savings Account",
        "A flat 2% nominal yield",
        "Fixed",
        0.02,
        0.0,
        None,
    ),
    ("No Growth", "Holds nominal value", "None", 0.0, 0.0, None),
];

/// 2024 US federal brackets, single filer.
const FEDERAL_BRACKETS: &[(f64, f64)] = &[
    (0.0, 0.10),
    (11_600.0, 0.12),
    (47_150.0, 0.22),
    (100_525.0, 0.24),
    (191_950.0, 0.32),
    (243_725.0, 0.35),
    (609_350.0, 0.37),
];

pub async fn seed_user_library(db: &Db, user_id: &str) -> ApiResult<()> {
    let mut tx = db.begin().await?;

    for (name, description, kind, mean, std_dev, asset_class) in RETURN_PROFILES {
        let (rate, mean_col, std_col) = match *kind {
            "Fixed" => (Some(*mean), None, None),
            "Normal" => (None, Some(*mean), Some(*std_dev)),
            _ => (None, None, None),
        };

        let distribution_id: i64 = sqlx::query_scalar(
            "INSERT INTO distributions (user_id, kind, rate, mean, std_dev)
             VALUES (?1,?2,?3,?4,?5) RETURNING id",
        )
        .bind(user_id)
        .bind(kind)
        .bind(rate)
        .bind(mean_col)
        .bind(std_col)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO return_profiles
                (user_id, name, description, distribution_id, asset_class)
             VALUES (?1,?2,?3,?4,?5)",
        )
        .bind(user_id)
        .bind(name)
        .bind(description)
        .bind(distribution_id)
        .bind(asset_class.map(AssetClass::as_str))
        .execute(&mut *tx)
        .await?;
    }

    // Inflation: a fixed long-run rate, and a stochastic alternative.
    for (name, description, kind, mean, std_dev) in [
        (
            "US Historical (fixed)",
            "CPI-U 1948-2025 geometric mean, 3.43%",
            "Fixed",
            0.0343436,
            0.0,
        ),
        (
            "US Historical (stochastic)",
            "CPI-U 1948-2025: 3.47% mean, 2.79% sd",
            "Normal",
            0.0347068,
            0.0279436,
        ),
    ] {
        let (rate, mean_col, std_col) = match kind {
            "Fixed" => (Some(mean), None, None),
            _ => (None, Some(mean), Some(std_dev)),
        };

        let distribution_id: i64 = sqlx::query_scalar(
            "INSERT INTO distributions (user_id, kind, rate, mean, std_dev)
             VALUES (?1,?2,?3,?4,?5) RETURNING id",
        )
        .bind(user_id)
        .bind(kind)
        .bind(rate)
        .bind(mean_col)
        .bind(std_col)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO inflation_profiles (user_id, name, description, distribution_id)
             VALUES (?1,?2,?3,?4)",
        )
        .bind(user_id)
        .bind(name)
        .bind(description)
        .bind(distribution_id)
        .execute(&mut *tx)
        .await?;
    }

    let tax_config_id: i64 = sqlx::query_scalar(
        "INSERT INTO tax_configs
            (user_id, name, description, state_rate, capital_gains_rate,
             early_withdrawal_penalty_rate)
         VALUES (?1, 'US Federal 2024 (single)', '2024 federal brackets, 5% state, 15% LTCG',
                 0.05, 0.15, 0.10)
         RETURNING id",
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;

    for (threshold, rate) in FEDERAL_BRACKETS {
        sqlx::query("INSERT INTO tax_brackets (tax_config_id, threshold, rate) VALUES (?1,?2,?3)")
            .bind(tax_config_id)
            .bind(threshold)
            .bind(rate)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(())
}
