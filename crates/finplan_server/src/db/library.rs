//! A user's library (return profiles, inflation profiles, tax configs) as
//! `finplan_plan`'s [`Library`] value: read it out of SQLite, or write one in.

use std::collections::HashMap;

use finplan_plan::graph::{DistributionRow, TaxBracketRow};
use finplan_plan::library::{
    Library, LibraryInflationProfile, LibraryReturnProfile, LibraryTaxConfig,
};
use finplan_plan::specs::taxes::Bracket;
use sqlx::{Sqlite, SqliteConnection, Transaction};

use crate::db::Db;
use crate::error::{ApiError, ApiResult};

/// The `tax_configs` columns a [`finplan_plan::graph::TaxConfigRow`] reads.
/// Its capital gains schedule is another table: [`gain_brackets`].
pub const TAX_CONFIG_COLUMNS: &str = "id, name, state_rate, early_withdrawal_penalty_rate,
     standard_deduction, age_65_extra_deduction, niit_threshold";

/// `tax_config_id`'s capital gains schedule, ascending.
pub async fn gain_brackets<'e>(
    db: impl sqlx::Executor<'e, Database = Sqlite>,
    tax_config_id: i64,
) -> sqlx::Result<Vec<TaxBracketRow>> {
    sqlx::query_as(
        "SELECT threshold, rate FROM tax_gain_brackets
          WHERE tax_config_id = ?1 ORDER BY threshold ASC",
    )
    .bind(tax_config_id)
    .fetch_all(db)
    .await
}

/// Write `brackets` as `tax_config_id`'s capital gains schedule, in place of
/// any it had.
pub async fn replace_gain_brackets(
    conn: &mut SqliteConnection,
    tax_config_id: i64,
    brackets: Vec<(f64, f64)>,
) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM tax_gain_brackets WHERE tax_config_id = ?1")
        .bind(tax_config_id)
        .execute(&mut *conn)
        .await?;
    for (threshold, rate) in brackets {
        sqlx::query(
            "INSERT INTO tax_gain_brackets (tax_config_id, threshold, rate) VALUES (?1,?2,?3)",
        )
        .bind(tax_config_id)
        .bind(threshold)
        .bind(rate)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

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
    let configs: Vec<(i64, String, Option<String>, f64, f64, f64, f64, Option<f64>)> =
        sqlx::query_as(
            "SELECT id, name, description, state_rate, early_withdrawal_penalty_rate,
                    standard_deduction, age_65_extra_deduction, niit_threshold
               FROM tax_configs WHERE user_id = ?1 ORDER BY id",
        )
        .bind(user_id)
        .fetch_all(&mut *conn)
        .await?;
    let mut tax_configs = Vec::with_capacity(configs.len());
    for (id, name, description, state, early, deduction, extra, niit) in configs {
        let brackets: Vec<(f64, f64)> = sqlx::query_as(
            "SELECT threshold, rate FROM tax_brackets WHERE tax_config_id = ?1 ORDER BY threshold",
        )
        .bind(id)
        .fetch_all(&mut *conn)
        .await?;
        let gains = gain_brackets(&mut *conn, id).await?;
        tax_configs.push(LibraryTaxConfig {
            id,
            name,
            description,
            state_rate: state,
            early_withdrawal_penalty_rate: early,
            standard_deduction: deduction,
            age_65_extra_deduction: extra,
            niit_threshold: niit,
            federal_brackets: brackets
                .into_iter()
                .map(|(threshold, rate)| Bracket { threshold, rate })
                .collect(),
            capital_gains_brackets: gains
                .into_iter()
                .map(|b| Bracket {
                    threshold: b.threshold,
                    rate: b.rate,
                })
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
                (user_id, name, description, state_rate, early_withdrawal_penalty_rate,
                 standard_deduction, age_65_extra_deduction, niit_threshold)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             RETURNING id",
        )
        .bind(user_id)
        .bind(&config.name)
        .bind(&config.description)
        .bind(config.state_rate)
        .bind(config.early_withdrawal_penalty_rate)
        .bind(config.standard_deduction)
        .bind(config.age_65_extra_deduction)
        .bind(config.niit_threshold)
        .fetch_one(&mut **tx)
        .await?;
        replace_gain_brackets(
            tx,
            id,
            config
                .capital_gains_brackets
                .iter()
                .map(|b| (b.threshold, b.rate))
                .collect(),
        )
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
