use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;

pub async fn init(database_url: &str) -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .expect("Failed to connect to SQLite database");

    migrate(&pool).await;
    pool
}

async fn migrate(pool: &SqlitePool) {
    // Enable WAL mode and foreign keys
    sqlx::query("PRAGMA journal_mode=WAL")
        .execute(pool)
        .await
        .expect("Failed to set WAL mode");

    sqlx::query("PRAGMA foreign_keys=ON")
        .execute(pool)
        .await
        .expect("Failed to enable foreign keys");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id             TEXT PRIMARY KEY,
            email          TEXT NOT NULL UNIQUE COLLATE NOCASE,
            password_hash  TEXT NOT NULL,
            created_at     TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await
    .expect("Failed to create users table");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS accounts (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id       TEXT NOT NULL REFERENCES users(id),
            name          TEXT NOT NULL,
            description   TEXT,
            account_type  TEXT NOT NULL,
            category      TEXT NOT NULL,
            value         REAL,
            return_profile TEXT,
            balance       REAL,
            interest_rate REAL,
            sort_order    INTEGER NOT NULL DEFAULT 0,
            created_at    TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await
    .expect("Failed to create accounts table");

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_accounts_user ON accounts(user_id)")
        .execute(pool)
        .await
        .expect("Failed to create accounts user index");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS holdings (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id  INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
            asset_name  TEXT NOT NULL,
            value       REAL NOT NULL DEFAULT 0.0,
            sort_order  INTEGER NOT NULL DEFAULT 0,
            created_at  TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await
    .expect("Failed to create holdings table");

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_holdings_account ON holdings(account_id)")
        .execute(pool)
        .await
        .expect("Failed to create holdings index");
}
