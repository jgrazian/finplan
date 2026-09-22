//! SQLite connection pool and migrations.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use sqlx::migrate::MigrateError;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Connection, SqliteConnection};
use thiserror::Error;

pub type Db = sqlx::SqlitePool;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Debug)]
pub struct RebuildReport {
    pub tables: usize,
    pub rows: u64,
}

#[derive(Debug, Error)]
pub enum RebuildError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sql(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] MigrateError),
}

#[derive(Debug, Eq, PartialEq, sqlx::FromRow)]
struct Column {
    name: String,
    type_name: String,
    not_null: i64,
    primary_key: i64,
}

/// Open (creating if needed) the SQLite database and run pending migrations.
///
/// WAL plus a busy timeout lets the simulation workers write progress while API
/// requests read concurrently; `foreign_keys` is per-connection in SQLite, so it
/// is set in the connect options rather than once at startup.
pub async fn connect(url: &str, max_connections: u32) -> Result<Db, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(10));

    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(15))
        .connect_with(options)
        .await?;

    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// Copy an existing database into a fresh file created by the consolidated
/// baseline migration.
///
/// The source is attached read-only by convention: no migration is run against
/// it and no statement writes to it. The destination is written under a unique
/// temporary name, checked for schema compatibility, row counts, foreign-key
/// violations, and SQLite integrity, then atomically published into place. An
/// existing destination is never overwritten.
pub async fn rebuild(
    source: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> Result<RebuildReport, RebuildError> {
    let source = source.as_ref().canonicalize()?;
    let destination = absolute_path(destination.as_ref())?;
    if source == destination {
        return Err(RebuildError::Invalid(
            "source and destination must be different paths".into(),
        ));
    }
    if destination.exists() {
        return Err(RebuildError::Invalid(format!(
            "destination already exists: {}",
            destination.display()
        )));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| RebuildError::Invalid("destination must have a parent directory".into()))?;
    if !parent.is_dir() {
        return Err(RebuildError::Invalid(format!(
            "destination directory does not exist: {}",
            parent.display()
        )));
    }
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| RebuildError::Invalid("destination must have a file name".into()))?;
    let temporary = parent.join(format!(".{file_name}.rebuild-{}.tmp", uuid::Uuid::new_v4()));

    let result = rebuild_into(&source, &temporary).await;
    match result {
        Ok(report) => {
            // Linking publishes without the overwrite behavior of rename(2).
            // Both paths live in the destination directory, so this is atomic
            // and cannot cross filesystems.
            if let Err(error) = std::fs::hard_link(&temporary, &destination) {
                remove_database_files(&temporary);
                return Err(error.into());
            }
            if let Err(error) = std::fs::remove_file(&temporary) {
                tracing::warn!(path = %temporary.display(), %error, "could not unlink rebuild temporary file");
            }
            Ok(report)
        }
        Err(error) => {
            remove_database_files(&temporary);
            Err(error)
        }
    }
}

async fn rebuild_into(source: &Path, destination: &Path) -> Result<RebuildReport, RebuildError> {
    let options = SqliteConnectOptions::new()
        .filename(destination)
        .create_if_missing(true)
        .foreign_keys(false)
        .busy_timeout(Duration::from_secs(30));
    let mut connection = SqliteConnection::connect_with(&options).await?;
    MIGRATOR.run(&mut connection).await?;
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut connection)
        .await?;
    sqlx::query("ATTACH DATABASE ?1 AS legacy")
        .bind(source.to_string_lossy().as_ref())
        .execute(&mut connection)
        .await?;

    verify_integrity(&mut connection, "legacy").await?;
    let tables = compatible_tables(&mut connection).await?;
    let mut copied_rows = 0u64;
    let mut transaction = connection.begin().await?;

    for (table, columns) in &tables {
        let retain_current_details = matches!(table.as_str(), "run_cash_flows" | "run_ledger");
        let table = quote_identifier(table);
        let columns = columns
            .iter()
            .map(|column| quote_identifier(&column.name))
            .collect::<Vec<_>>()
            .join(", ");
        // These path details intentionally exist only for the newest successful
        // run in a scenario. Filtering while copying also upgrades a migration
        // 15 database without first mutating it with the retired migration 16.
        let retained = if retain_current_details {
            " WHERE run_id IN (
                    SELECT MAX(id) FROM legacy.runs
                     WHERE status = 'succeeded' GROUP BY scenario_id
                  )"
        } else {
            ""
        };
        let copied = sqlx::query(&format!(
            "INSERT INTO main.{table} ({columns})
             SELECT {columns} FROM legacy.{table}{retained}"
        ))
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        let expected: i64 =
            sqlx::query_scalar(&format!("SELECT COUNT(*) FROM legacy.{table}{retained}"))
                .fetch_one(&mut *transaction)
                .await?;
        if copied != expected as u64 {
            return Err(RebuildError::Invalid(format!(
                "copied {copied} of {expected} rows from {table}"
            )));
        }
        copied_rows += copied;
    }

    transaction.commit().await?;
    verify_integrity(&mut connection, "main").await?;
    sqlx::query("DETACH DATABASE legacy")
        .execute(&mut connection)
        .await?;
    connection.close().await?;

    Ok(RebuildReport {
        tables: tables.len(),
        rows: copied_rows,
    })
}

async fn compatible_tables(
    connection: &mut SqliteConnection,
) -> Result<Vec<(String, Vec<Column>)>, RebuildError> {
    let sql = "SELECT name FROM {schema}.sqlite_schema
                WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
                  AND name <> '_sqlx_migrations' ORDER BY name";
    let target: Vec<String> = sqlx::query_scalar(&sql.replace("{schema}", "main"))
        .fetch_all(&mut *connection)
        .await?;
    let source: Vec<String> = sqlx::query_scalar(&sql.replace("{schema}", "legacy"))
        .fetch_all(&mut *connection)
        .await?;
    if target != source {
        let missing = target
            .iter()
            .filter(|table| !source.contains(table))
            .cloned()
            .collect::<Vec<_>>();
        let extra = source
            .iter()
            .filter(|table| !target.contains(table))
            .cloned()
            .collect::<Vec<_>>();
        return Err(RebuildError::Invalid(format!(
            "source schema is incompatible; missing tables: {missing:?}; extra tables: {extra:?}"
        )));
    }

    let mut tables = Vec::with_capacity(target.len());
    for table in target {
        let mut target_columns = table_columns(connection, "main", &table).await?;
        let mut source_columns = table_columns(connection, "legacy", &table).await?;
        target_columns.sort_by(|left, right| left.name.cmp(&right.name));
        source_columns.sort_by(|left, right| left.name.cmp(&right.name));
        if target_columns != source_columns {
            return Err(RebuildError::Invalid(format!(
                "source table {table:?} does not match the consolidated schema"
            )));
        }
        // The INSERT names its columns, so legacy ALTER TABLE column order does
        // not need to match the cleaner consolidated declaration order.
        tables.push((table, target_columns));
    }
    Ok(tables)
}

async fn table_columns(
    connection: &mut SqliteConnection,
    schema: &str,
    table: &str,
) -> Result<Vec<Column>, sqlx::Error> {
    sqlx::query_as(
        "SELECT name, type AS type_name, \"notnull\" AS not_null, pk AS primary_key
           FROM pragma_table_info(?1, ?2)",
    )
    .bind(table)
    .bind(schema)
    .fetch_all(connection)
    .await
}

async fn verify_integrity(
    connection: &mut SqliteConnection,
    schema: &str,
) -> Result<(), RebuildError> {
    let result: String = sqlx::query_scalar(&format!("PRAGMA {schema}.integrity_check"))
        .fetch_one(&mut *connection)
        .await?;
    if result != "ok" {
        return Err(RebuildError::Invalid(format!(
            "{schema} database integrity check failed: {result}"
        )));
    }
    let violations: Vec<(String, Option<i64>, String, i64)> =
        sqlx::query_as(&format!("PRAGMA {schema}.foreign_key_check"))
            .fetch_all(&mut *connection)
            .await?;
    if !violations.is_empty() {
        return Err(RebuildError::Invalid(format!(
            "{schema} database has {} foreign-key violation(s)",
            violations.len()
        )));
    }
    Ok(())
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn absolute_path(path: &Path) -> Result<PathBuf, std::io::Error> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn remove_database_files(path: &Path) {
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-shm", path.display())),
        PathBuf::from(format!("{}-wal", path.display())),
    ] {
        if let Err(error) = std::fs::remove_file(&candidate)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(path = %candidate.display(), %error, "could not remove incomplete rebuild");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rebuild_preserves_data_resets_history_and_prunes_old_details() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("legacy.db");
        let destination_path = directory.path().join("v0.db");
        let source = connect(&format!("sqlite://{}", source_path.display()), 1)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO users(id,email,password_hash) VALUES('user','user@example.com','hash')",
        )
        .execute(&source)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO scenarios(id,user_id,name,start_date)
             VALUES(1,'user','Plan','2026-01-01')",
        )
        .execute(&source)
        .await
        .unwrap();
        for run_id in [1, 2] {
            sqlx::query(
                "INSERT INTO runs(id,scenario_id,user_id,status,iterations)
                 VALUES(?1,1,'user','succeeded',10)",
            )
            .bind(run_id)
            .execute(&source)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO run_cash_flows(run_id,percentile,year)
                 VALUES(?1,0.5,2026)",
            )
            .bind(run_id)
            .execute(&source)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO run_ledger(run_id,percentile,position,as_of_date,year,category,kind,detail)
                 VALUES(?1,0.5,0,'2026-01-01',2026,'cash','Income','Salary')",
            )
            .bind(run_id)
            .execute(&source)
            .await
            .unwrap();
        }
        source.close().await;

        let report = rebuild(&source_path, &destination_path).await.unwrap();
        assert_eq!(report.tables, 45);

        let rebuilt = connect(&format!("sqlite://{}", destination_path.display()), 1)
            .await
            .unwrap();
        let migration_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&rebuilt)
            .await
            .unwrap();
        assert_eq!(migration_count, 2);
        for table in ["run_cash_flows", "run_ledger"] {
            let ids: Vec<i64> = sqlx::query_scalar(&format!(
                "SELECT DISTINCT run_id FROM {table} ORDER BY run_id"
            ))
            .fetch_all(&rebuilt)
            .await
            .unwrap();
            assert_eq!(ids, vec![2], "{table} retained an older run");
        }
        let runs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs")
            .fetch_one(&rebuilt)
            .await
            .unwrap();
        assert_eq!(runs, 2, "run history itself must survive the rebuild");
        rebuilt.close().await;

        let mut untouched = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&source_path)
                .foreign_keys(true),
        )
        .await
        .unwrap();
        let old_details: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM run_ledger")
            .fetch_one(&mut untouched)
            .await
            .unwrap();
        assert_eq!(old_details, 2, "the source database was modified");
        untouched.close().await.unwrap();
    }
}
