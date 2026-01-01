use rusqlite::Connection;

pub fn init_db(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS simulations (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            parameters TEXT NOT NULL,
            portfolio_id TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS simulation_runs (
            id TEXT PRIMARY KEY,
            simulation_id TEXT NOT NULL,
            result TEXT NOT NULL,
            iterations INTEGER NOT NULL,
            ran_at TEXT NOT NULL,
            FOREIGN KEY (simulation_id) REFERENCES simulations(id) ON DELETE CASCADE
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS portfolios (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            accounts TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        [],
    )?;

    Ok(())
}
