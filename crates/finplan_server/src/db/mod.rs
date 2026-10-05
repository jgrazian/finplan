//! SQLite connection pool and migrations.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use sqlx::migrate::MigrateError;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Connection, SqliteConnection};
use thiserror::Error;

pub(crate) mod batch;
pub mod graph;
pub mod library;

pub type Db = sqlx::SqlitePool;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Per-path series and itemised detail, kept only for the newest successful run
/// in each scenario. Older runs keep their summary (`run_stats`,
/// `run_percentile_values`, `run_real_stats`), which is all a history or
/// comparison view needs. Only tables written when a run succeeds belong here:
/// `run_account_labels` is captured at enqueue, so pruning it would strip a
/// newer run that is still in flight.
pub const CURRENT_RUN_ONLY_TABLES: [&str; 8] = [
    "run_net_worth_points",
    "run_account_points",
    "run_cash_flows",
    "run_taxes",
    "run_inflation",
    "run_warnings",
    "run_real_quantiles",
    "run_ledger",
];

/// The seed of the run's median market path: the stored percentile closest to
/// 0.5 (the lower one on a tie). None for a run saved before seeds were kept.
pub async fn median_seed(db: &Db, run_id: i64) -> Result<Option<u64>, sqlx::Error> {
    let seed: Option<String> = sqlx::query_scalar(
        "SELECT seed FROM run_percentiles WHERE run_id = ?1 AND seed IS NOT NULL
          ORDER BY abs(percentile - 0.5), percentile LIMIT 1",
    )
    .bind(run_id)
    .fetch_optional(db)
    .await?;
    Ok(seed.and_then(|s| s.parse().ok()))
}

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
        let retain_current_details = CURRENT_RUN_ONLY_TABLES.contains(&table.as_str());
        let table = quote_identifier(table);
        let columns = columns
            .iter()
            .map(|column| quote_identifier(&column.name))
            .collect::<Vec<_>>()
            .join(", ");
        // Path details intentionally exist only for the newest successful run
        // in a scenario. Filtering while copying also upgrades a migration
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

    /// Migration 0025 rebuilds `effects` inside sqlx's transaction, with
    /// foreign keys on: every effect, withdrawal source and Random branch on
    /// an existing plan must survive it.
    #[tokio::test]
    async fn the_roth_conversion_migration_keeps_every_effect_and_its_children() {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(true);
        let mut conn = SqliteConnection::connect_with(&options).await.unwrap();
        let before = sqlx::migrate::Migrator {
            migrations: std::borrow::Cow::Owned(
                MIGRATOR
                    .iter()
                    .filter(|m| m.version < 25)
                    .cloned()
                    .collect(),
            ),
            ignore_missing: false,
            locking: true,
            no_tx: false,
        };
        before.run(&mut conn).await.unwrap();
        for statement in [
            "INSERT INTO users(id,email,password_hash) VALUES('u','u@example.com','hash')",
            "INSERT INTO scenarios(id,user_id,name,start_date) VALUES(1,'u','Plan','2026-01-01')",
            "INSERT INTO accounts(id,scenario_id,name,flavor) VALUES(1,1,'IRA','Investment'),
                 (2,1,'Checking','Bank')",
            "INSERT INTO events(id,scenario_id,name) VALUES(1,1,'Draw'),(2,1,'Maybe')",
            "INSERT INTO transfer_amounts(id,scenario_id,kind,value) VALUES(1,1,'Fixed',100),
                 (2,1,'Fixed',5)",
            "INSERT INTO effects(id,scenario_id,event_id,position,kind,to_account_id,amount_id,
                 income_type) VALUES(1,1,1,0,'Sweep',2,1,'Taxable')",
            "INSERT INTO effect_withdrawal_sources(effect_id,mode,account_id)
                 VALUES(1,'SingleAccount',1)",
            "INSERT INTO effect_withdrawal_source_items(effect_id,role,position,account_id)
                 VALUES(1,'exclude',0,2)",
            "INSERT INTO effects(id,scenario_id,event_id,position,kind,probability)
                 VALUES(2,1,2,0,'Random',0.5)",
            "INSERT INTO effects(id,scenario_id,parent_id,parent_slot,kind,from_account_id,
                 amount_id) VALUES(3,1,2,'on_true','Expense',2,2)",
        ] {
            sqlx::query(statement).execute(&mut conn).await.unwrap();
        }

        MIGRATOR.run(&mut conn).await.unwrap();

        let count = |sql: &'static str| sqlx::query_scalar::<_, i64>(sql);
        assert_eq!(
            count("SELECT COUNT(*) FROM effects")
                .fetch_one(&mut conn)
                .await
                .unwrap(),
            3
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM effect_withdrawal_sources")
                .fetch_one(&mut conn)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM effect_withdrawal_source_items")
                .fetch_one(&mut conn)
                .await
                .unwrap(),
            1
        );
        let violations: Vec<(String,)> =
            sqlx::query_as("SELECT \"table\" FROM pragma_foreign_key_check")
                .fetch_all(&mut conn)
                .await
                .unwrap();
        assert!(violations.is_empty(), "{violations:?}");
        // The self-reference was renamed with the table: deleting the Random
        // effect still takes its branch with it.
        let parent_table: String = sqlx::query_scalar(
            "SELECT \"table\" FROM pragma_foreign_key_list('effects') WHERE \"from\" = 'parent_id'",
        )
        .fetch_one(&mut conn)
        .await
        .unwrap();
        assert_eq!(parent_table, "effects");
        sqlx::query("DELETE FROM effects WHERE id = 2")
            .execute(&mut conn)
            .await
            .unwrap();
        assert_eq!(
            count("SELECT COUNT(*) FROM effects")
                .fetch_one(&mut conn)
                .await
                .unwrap(),
            1
        );
        // And the new kind and column are accepted.
        sqlx::query(
            "INSERT INTO accounts(id,scenario_id,name,flavor) VALUES(3,1,'Roth','Investment')",
        )
        .execute(&mut conn)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO effects(scenario_id,event_id,position,kind,from_account_id,to_account_id,
                 amount_id,pay_tax_from_account_id) VALUES(1,2,1,'RothConversion',1,3,2,2)",
        )
        .execute(&mut conn)
        .await
        .unwrap();
    }

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
        sqlx::query(
            "INSERT INTO named_parameters(id,scenario_id,name,kind,number_value)
             VALUES(1,1,'Spending','Money',2500)",
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
        assert_eq!(report.tables, 60);

        let rebuilt = connect(&format!("sqlite://{}", destination_path.display()), 1)
            .await
            .unwrap();
        let migration_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&rebuilt)
            .await
            .unwrap();
        assert_eq!(migration_count as usize, MIGRATOR.iter().count());
        let parameter_value: f64 = sqlx::query_scalar(
            "SELECT number_value FROM named_parameters WHERE scenario_id=1 AND name='Spending'",
        )
        .fetch_one(&rebuilt)
        .await
        .unwrap();
        assert_eq!(parameter_value, 2500.0);
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
    /// Apply migrations `from < version <= to`, each in a transaction as the
    /// real migrator does.
    async fn apply(conn: &mut SqliteConnection, from: i64, to: i64) {
        for m in MIGRATOR
            .iter()
            .filter(|m| m.version > from && m.version <= to)
        {
            let mut tx = sqlx::Connection::begin(&mut *conn).await.unwrap();
            sqlx::raw_sql(&m.sql).execute(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
        }
    }

    /// A database at the 0009 schema, with review notes, chat threads and a
    /// note parented to another, upgrades through 0010-0012 (which rebuild
    /// `suggestions`) without losing a row, an index or a link, and with the
    /// foreign keys on, as the real migrator runs them.
    #[tokio::test]
    async fn drafts_migrations_upgrade_a_populated_database() {
        let mut conn = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .in_memory(true)
                .foreign_keys(true),
        )
        .await
        .unwrap();
        apply(&mut conn, 0, 9).await;

        for sql in [
            "INSERT INTO users(id,email,password_hash) VALUES('u','u@example.com','h')",
            "INSERT INTO scenarios(id,user_id,name,start_date) VALUES(1,'u','Plan','2026-01-01')",
            "INSERT INTO runs(id,scenario_id,user_id,status,iterations) VALUES(1,1,'u','succeeded',10)",
        ] {
            sqlx::query(sql).execute(&mut conn).await.unwrap();
        }
        for (id, status, parent) in [
            (1, "open", None),
            (2, "applied", Some(1)),
            (3, "dismissed", Some(2)),
        ] {
            sqlx::query(
                "INSERT INTO suggestions (id, scenario_id, run_id, source, rule, kind, section,
                     title, reasoning, evidence_json, fingerprint, status, review_job,
                     paths_json, applied_path, created_json, parent_id, resolved_at)
                 VALUES (?1, 1, 1, 'ai', NULL, 'fix', 'plan', ?2, 'why', '[]', ?3, ?4, 'job',
                         '[{\"key\":\"a\"}]', 'a', '{\"k\":1}', ?5, '2026-09-28 10:00:00')",
            )
            .bind(id)
            .bind(format!("Note {id}"))
            .bind(format!("f{id}"))
            .bind(status)
            .bind(parent)
            .execute(&mut conn)
            .await
            .unwrap();
        }
        for sql in [
            "INSERT INTO suggestion_threads(suggestion_id,status,job,turns) VALUES(2,'running','j',3)",
            "INSERT INTO suggestion_messages(suggestion_id,role,text,suggestion_ids) VALUES(2,'user','hi','[]')",
            "INSERT INTO suggestion_messages(suggestion_id,role,text,suggestion_ids) VALUES(2,'assistant','yo','[3]')",
        ] {
            sqlx::query(sql).execute(&mut conn).await.unwrap();
        }

        apply(&mut conn, 9, 12).await;

        type Note = (
            i64,
            i64,
            String,
            String,
            Option<i64>,
            Option<String>,
            String,
            i64,
        );
        let rows: Vec<Note> = sqlx::query_as(
            "SELECT id, run_id, title, status, parent_id, applied_path, created_json, auto_added
                   FROM suggestions ORDER BY id",
        )
        .fetch_all(&mut conn)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![
                (
                    1,
                    1,
                    "Note 1".into(),
                    "open".into(),
                    None,
                    Some("a".into()),
                    "{\"k\":1}".into(),
                    0
                ),
                (
                    2,
                    1,
                    "Note 2".into(),
                    "applied".into(),
                    Some(1),
                    Some("a".into()),
                    "{\"k\":1}".into(),
                    0
                ),
                (
                    3,
                    1,
                    "Note 3".into(),
                    "dismissed".into(),
                    Some(2),
                    Some("a".into()),
                    "{\"k\":1}".into(),
                    0
                ),
            ],
            "every note and its parent link survive the rebuilds"
        );
        let thread: (i64, String, Option<i64>) =
            sqlx::query_as("SELECT suggestion_id, status, turns FROM suggestion_threads")
                .fetch_one(&mut conn)
                .await
                .unwrap();
        assert_eq!(thread, (2, "running".into(), Some(3)));
        let messages: Vec<(String, String)> =
            sqlx::query_as("SELECT role, suggestion_ids FROM suggestion_messages ORDER BY id")
                .fetch_all(&mut conn)
                .await
                .unwrap();
        assert_eq!(
            messages,
            vec![
                ("user".into(), "[]".into()),
                ("assistant".into(), "[3]".into())
            ]
        );

        let indexes: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='suggestions'
                AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .fetch_all(&mut conn)
        .await
        .unwrap();
        assert_eq!(
            indexes,
            ["suggestions_by_fingerprint", "suggestions_by_scenario"]
        );
        let broken: Vec<(String,)> = sqlx::query_as("PRAGMA foreign_key_check")
            .fetch_all(&mut conn)
            .await
            .unwrap();
        assert!(broken.is_empty(), "dangling foreign keys: {broken:?}");

        // The rebuilt table takes what the drafts need and still cascades.
        sqlx::query(
            "INSERT INTO suggestions (scenario_id, run_id, source, kind, section, title, reasoning,
                 evidence_json, fingerprint, note_key, blocked_by_json, board_column)
             VALUES (1, NULL, 'ai', 'add', 'plan', 't', 'r', '[]', 'f', 'k', '[\"q\"]', 'to_confirm')",
        )
        .execute(&mut conn)
        .await
        .unwrap();
        sqlx::query("DELETE FROM scenarios WHERE id = 1")
            .execute(&mut conn)
            .await
            .unwrap();
        for table in ["suggestions", "suggestion_threads", "suggestion_messages"] {
            let left: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&mut conn)
                .await
                .unwrap();
            assert_eq!(left, 0, "{table} outlived its scenario");
        }
    }
}
