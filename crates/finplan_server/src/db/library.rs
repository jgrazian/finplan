//! A user's library (return profiles, inflation profiles, tax configs) as
//! `finplan_plan`'s [`Library`] value: read it out of SQLite, or write one in.

use std::collections::HashMap;

use finplan_plan::graph::DistributionRow;
use finplan_plan::library::{
    Library, LibraryInflationProfile, LibraryReturnProfile, LibraryTaxConfig,
};
use finplan_plan::specs::taxes::Bracket;
use sqlx::{Sqlite, SqliteConnection, Transaction};

use crate::db::Db;
use crate::error::{ApiError, ApiResult};

/// Everything in `user_id`'s library, every list in id order.
pub async fn load(db: &Db, user_id: &str) -> ApiResult<Library> {
    let mut conn = db.acquire().await?;
    load_connection(&mut conn, user_id).await
}

pub async fn load_connection(conn: &mut SqliteConnection, user_id: &str) -> ApiResult<Library> {
    let return_profiles: Vec<LibraryReturnProfile> = sqlx::query_as::<
        _,
        (i64, String, Option<String>, Option<String>, i64, i64),
    >(
        "SELECT id, name, description, asset_class, distribution_id, sort_order
           FROM return_profiles WHERE user_id = ?1 ORDER BY id",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(
        |(id, name, description, asset_class, distribution_id, sort_order)| LibraryReturnProfile {
            id,
            name,
            description,
            asset_class,
            distribution_id,
            sort_order,
        },
    )
    .collect();

    let inflation_profiles: Vec<LibraryInflationProfile> =
        sqlx::query_as::<_, (i64, String, Option<String>, i64, i64)>(
            "SELECT id, name, description, distribution_id, sort_order
               FROM inflation_profiles WHERE user_id = ?1 ORDER BY id",
        )
        .bind(user_id)
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(
            |(id, name, description, distribution_id, sort_order)| LibraryInflationProfile {
                id,
                name,
                description,
                distribution_id,
                sort_order,
            },
        )
        .collect();

    let distributions: Vec<DistributionRow> = sqlx::query_as(
        "SELECT id, kind, rate, mean, std_dev, scale, df, bull_id, bear_id,
                bull_to_bear_prob, bear_to_bull_prob, history_preset, block_size
           FROM distributions WHERE user_id = ?1 ORDER BY id",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;

    #[allow(clippy::type_complexity)]
    let configs: Vec<(i64, String, Option<String>, f64, f64, f64, f64, f64)> = sqlx::query_as(
        "SELECT id, name, description, state_rate, capital_gains_rate,
                early_withdrawal_penalty_rate, standard_deduction, age_65_extra_deduction
           FROM tax_configs WHERE user_id = ?1 ORDER BY id",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut tax_configs = Vec::with_capacity(configs.len());
    for (id, name, description, state, gains, early, deduction, extra) in configs {
        let brackets: Vec<(f64, f64)> = sqlx::query_as(
            "SELECT threshold, rate FROM tax_brackets WHERE tax_config_id = ?1 ORDER BY threshold",
        )
        .bind(id)
        .fetch_all(&mut *conn)
        .await?;
        tax_configs.push(LibraryTaxConfig {
            id,
            name,
            description,
            state_rate: state,
            capital_gains_rate: gains,
            early_withdrawal_penalty_rate: early,
            standard_deduction: deduction,
            age_65_extra_deduction: extra,
            federal_brackets: brackets
                .into_iter()
                .map(|(threshold, rate)| Bracket { threshold, rate })
                .collect(),
        });
    }

    Ok(Library {
        return_profiles,
        inflation_profiles,
        tax_configs,
        distributions,
    })
}

/// Write `library` as `user_id`'s rows, which are given new ids: distributions
/// first (in id order, so a regime's children precede it), then the profiles
/// that point at them, then the tax configs and their brackets.
pub async fn insert(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    library: &Library,
) -> ApiResult<()> {
    let mut distributions: HashMap<i64, i64> = HashMap::new();
    let mut rows: Vec<&DistributionRow> = library.distributions.iter().collect();
    rows.sort_by_key(|d| d.id);
    for row in rows {
        let mapped = |id: Option<i64>| -> ApiResult<Option<i64>> {
            id.map(|id| {
                distributions.get(&id).copied().ok_or_else(|| {
                    ApiError::internal(format!("distribution {id} is used before it is defined"))
                })
            })
            .transpose()
        };
        let (bull_id, bear_id) = (mapped(row.bull_id)?, mapped(row.bear_id)?);
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO distributions
                (user_id, kind, rate, mean, std_dev, scale, df, bull_id, bear_id,
                 bull_to_bear_prob, bear_to_bull_prob, history_preset, block_size)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13) RETURNING id",
        )
        .bind(user_id)
        .bind(&row.kind)
        .bind(row.rate)
        .bind(row.mean)
        .bind(row.std_dev)
        .bind(row.scale)
        .bind(row.df)
        .bind(bull_id)
        .bind(bear_id)
        .bind(row.bull_to_bear_prob)
        .bind(row.bear_to_bull_prob)
        .bind(&row.history_preset)
        .bind(row.block_size)
        .fetch_one(&mut **tx)
        .await?;
        distributions.insert(row.id, id);
    }
    let distribution = |id: i64| -> ApiResult<i64> {
        distributions.get(&id).copied().ok_or_else(|| {
            ApiError::internal(format!("distribution {id} is missing from the library"))
        })
    };

    for profile in &library.return_profiles {
        sqlx::query(
            "INSERT INTO return_profiles
                (user_id, name, description, distribution_id, asset_class, sort_order)
             VALUES (?1,?2,?3,?4,?5,?6)",
        )
        .bind(user_id)
        .bind(&profile.name)
        .bind(&profile.description)
        .bind(distribution(profile.distribution_id)?)
        .bind(&profile.asset_class)
        .bind(profile.sort_order)
        .execute(&mut **tx)
        .await?;
    }

    for profile in &library.inflation_profiles {
        sqlx::query(
            "INSERT INTO inflation_profiles
                (user_id, name, description, distribution_id, sort_order)
             VALUES (?1,?2,?3,?4,?5)",
        )
        .bind(user_id)
        .bind(&profile.name)
        .bind(&profile.description)
        .bind(distribution(profile.distribution_id)?)
        .bind(profile.sort_order)
        .execute(&mut **tx)
        .await?;
    }

    for config in &library.tax_configs {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO tax_configs
                (user_id, name, description, state_rate, capital_gains_rate,
                 early_withdrawal_penalty_rate, standard_deduction, age_65_extra_deduction)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             RETURNING id",
        )
        .bind(user_id)
        .bind(&config.name)
        .bind(&config.description)
        .bind(config.state_rate)
        .bind(config.capital_gains_rate)
        .bind(config.early_withdrawal_penalty_rate)
        .bind(config.standard_deduction)
        .bind(config.age_65_extra_deduction)
        .fetch_one(&mut **tx)
        .await?;
        for bracket in &config.federal_brackets {
            sqlx::query(
                "INSERT INTO tax_brackets (tax_config_id, threshold, rate) VALUES (?1,?2,?3)",
            )
            .bind(id)
            .bind(bracket.threshold)
            .bind(bracket.rate)
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}
