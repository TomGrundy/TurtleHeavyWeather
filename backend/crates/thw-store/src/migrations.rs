use chrono::{SecondsFormat, Utc};
use rusqlite::Connection;

use crate::StoreError;

/// A single versioned SQLite schema migration.
pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

/// Embedded schema migrations for `thw-store`.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "initial_schema",
    sql: r#"
CREATE TABLE IF NOT EXISTS nodes (
    node_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    state TEXT NOT NULL,
    lat REAL NOT NULL,
    lon REAL NOT NULL,
    bom_station_id TEXT,
    bom_product_id TEXT,
    bom_forecast_product TEXT,
    is_major_hub INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS forecast_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id TEXT NOT NULL REFERENCES nodes(node_id) ON DELETE CASCADE,
    source_id TEXT NOT NULL,
    issued_at TEXT NOT NULL,
    valid_at TEXT NOT NULL,
    lead_hours INTEGER NOT NULL,
    temp_c REAL,
    feels_like_c REAL,
    feels_like_is_derived INTEGER NOT NULL DEFAULT 0,
    rain_prob_pct REAL,
    rain_mm REAL,
    wind_kmh REAL,
    humidity_pct REAL,
    summary TEXT,
    UNIQUE(node_id, source_id, issued_at, valid_at)
);

CREATE INDEX IF NOT EXISTS idx_forecast_snapshots_valid_at
    ON forecast_snapshots(valid_at);
CREATE INDEX IF NOT EXISTS idx_forecast_snapshots_node_valid
    ON forecast_snapshots(node_id, valid_at);

CREATE TABLE IF NOT EXISTS aggregate_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id TEXT NOT NULL REFERENCES nodes(node_id) ON DELETE CASCADE,
    issued_at TEXT NOT NULL,
    valid_at TEXT NOT NULL,
    lead_hours INTEGER NOT NULL,
    feels_like_c REAL NOT NULL,
    temp_c REAL NOT NULL,
    rain_verdict TEXT NOT NULL,
    rain_prob_pct REAL NOT NULL,
    rain_mm_expected REAL NOT NULL,
    source_count INTEGER NOT NULL,
    weights_version TEXT NOT NULL,
    UNIQUE(node_id, issued_at, valid_at)
);

CREATE INDEX IF NOT EXISTS idx_aggregate_snapshots_valid_at
    ON aggregate_snapshots(valid_at);
CREATE INDEX IF NOT EXISTS idx_aggregate_snapshots_node_valid
    ON aggregate_snapshots(node_id, valid_at);

CREATE TABLE IF NOT EXISTS observations (
    node_id TEXT NOT NULL REFERENCES nodes(node_id) ON DELETE CASCADE,
    observed_at TEXT NOT NULL,
    source_id TEXT NOT NULL,
    quality_rank INTEGER NOT NULL DEFAULT 1,
    temp_c REAL,
    feels_like_c REAL,
    rain_mm_1h REAL,
    rained_bool INTEGER,
    wind_kmh REAL,
    humidity_pct REAL,
    PRIMARY KEY (node_id, observed_at, source_id)
);

CREATE INDEX IF NOT EXISTS idx_observations_observed_at
    ON observations(observed_at);

CREATE TABLE IF NOT EXISTS regression_metrics (
    computed_at TEXT NOT NULL,
    window_days INTEGER NOT NULL DEFAULT 7,
    subject_id TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    lead_bucket TEXT NOT NULL,
    sample_count INTEGER NOT NULL,
    temp_mae_c REAL,
    temp_rmse_c REAL,
    temp_bias_c REAL,
    feels_like_mae_c REAL,
    feels_like_rmse_c REAL,
    rain_brier_score REAL,
    rain_hit_rate REAL,
    PRIMARY KEY (subject_id, lead_bucket)
);

CREATE TABLE IF NOT EXISTS regression_meta (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    computed_at TEXT NOT NULL,
    window_days INTEGER NOT NULL DEFAULT 7,
    suggested_weights_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS node_bundles (
    node_id TEXT PRIMARY KEY REFERENCES nodes(node_id) ON DELETE CASCADE,
    generated_at TEXT NOT NULL,
    bundle_json TEXT NOT NULL
);
"#,
}];

/// Runs all unapplied embedded migrations in order.
pub fn run_migrations(conn: &mut Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );
        "#,
    )?;

    for migration in MIGRATIONS {
        let already_applied: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
            [migration.version],
            |row| row.get(0),
        )?;

        if already_applied {
            continue;
        }

        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)?;
        let applied_at = Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true);
        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![migration.version, migration.name, applied_at],
        )?;
        tx.commit()?;
    }

    Ok(())
}

/// Returns the highest applied migration version, or `0` if none have been applied.
pub fn current_schema_version(conn: &Connection) -> Result<i64, StoreError> {
    let version: Option<i64> =
        conn.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })?;
    Ok(version.unwrap_or(0))
}
