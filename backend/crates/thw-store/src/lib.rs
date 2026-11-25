pub mod migrations;

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::debug;

pub use thw_core;
use thw_core::{
    MeshNode, NodeWeatherBundle, ObservationRecord, RainVerdict, RegressionMetricRow,
    RegressionSummary, SourceReport, StateCode, WeatherPoint,
};

/// Subject identifier reserved for the TurtleHeavyWeather aggregate estimate in regression tables.
pub const AGGREGATE_SUBJECT_ID: &str = "__aggregate__";

/// Errors returned by [`WeatherStore`] operations.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid timestamp '{0}': {1}")]
    InvalidTimestamp(String, #[source] chrono::ParseError),
    #[error("invalid state code '{0}'")]
    InvalidStateCode(String),
    #[error("invalid stored data: {0}")]
    InvalidData(String),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
}

/// Row counts deleted during a retention pruning pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PruneStats {
    pub forecast_snapshots_deleted: usize,
    pub aggregate_snapshots_deleted: usize,
    pub observations_deleted: usize,
}

/// Unified historical forecast or aggregate snapshot row returned for regression scoring.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredForecastRow {
    pub node_id: String,
    pub subject_id: String,
    pub is_aggregate: bool,
    pub issued_at: DateTime<Utc>,
    pub valid_at: DateTime<Utc>,
    pub lead_hours: i64,
    pub temp_c: Option<f64>,
    pub feels_like_c: Option<f64>,
    pub rain_prob_pct: Option<f64>,
    pub rain_mm: Option<f64>,
}

/// Embedded SQLite weather store managing 7-day rolling snapshots, observations,
/// latest node bundles, and regression metrics.
pub struct WeatherStore {
    conn: Connection,
}

impl WeatherStore {
    /// Opens (or creates) a file-backed SQLite database in WAL mode and runs embedded migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let mut conn = Connection::open(path)?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            PRAGMA synchronous = NORMAL;
            PRAGMA busy_timeout = 5000;
            "#,
        )?;
        migrations::run_migrations(&mut conn)?;
        Ok(Self { conn })
    }

    /// Opens an existing file-backed SQLite database in read-only mode without running migrations.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(path.as_ref(), flags)?;
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = 5000;
            "#,
        )?;
        Ok(Self { conn })
    }

    /// Opens an isolated in-memory SQLite database and runs embedded migrations.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let mut conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        migrations::run_migrations(&mut conn)?;
        Ok(Self { conn })
    }

    /// Returns a reference to the underlying `rusqlite::Connection`.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Returns the highest applied schema migration version.
    pub fn schema_version(&self) -> Result<i64, StoreError> {
        migrations::current_schema_version(&self.conn)
    }

    /// Idempotently inserts or updates mesh nodes.
    pub fn upsert_nodes(&mut self, nodes: &[MeshNode]) -> Result<(), StoreError> {
        if nodes.is_empty() {
            return Ok(());
        }

        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                r#"
                INSERT INTO nodes (
                    node_id, name, state, lat, lon,
                    bom_station_id, bom_product_id, bom_forecast_product, is_major_hub
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                ON CONFLICT(node_id) DO UPDATE SET
                    name = excluded.name,
                    state = excluded.state,
                    lat = excluded.lat,
                    lon = excluded.lon,
                    bom_station_id = excluded.bom_station_id,
                    bom_product_id = excluded.bom_product_id,
                    bom_forecast_product = excluded.bom_forecast_product,
                    is_major_hub = excluded.is_major_hub
                "#,
            )?;

            for node in nodes {
                upsert_node_with_stmt(&mut stmt, node)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Lists all mesh nodes ordered by `node_id`.
    pub fn list_nodes(&self) -> Result<Vec<MeshNode>, StoreError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT node_id, name, state, lat, lon,
                   bom_station_id, bom_product_id, bom_forecast_product, is_major_hub
            FROM nodes
            ORDER BY node_id ASC
            "#,
        )?;

        let rows = stmt.query_map([], |row| {
            let state_raw: String = row.get(2)?;
            let is_major_hub_int: i64 = row.get(8)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                state_raw,
                row.get::<_, f64>(3)?,
                row.get::<_, f64>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                is_major_hub_int != 0,
            ))
        })?;

        let mut nodes = Vec::new();
        for row in rows {
            let (
                node_id,
                name,
                state_raw,
                lat,
                lon,
                bom_station_id,
                bom_product_id,
                bom_forecast_product,
                is_major_hub,
            ) = row?;
            nodes.push(MeshNode {
                node_id,
                name,
                state: parse_state_code(&state_raw)?,
                lat,
                lon,
                bom_station_id,
                bom_product_id,
                bom_forecast_product,
                is_major_hub,
            });
        }
        Ok(nodes)
    }

    /// Idempotently upserts `current` and `hourly` weather points from source reports
    /// into `forecast_snapshots` for `node_id`.
    pub fn upsert_source_reports(
        &mut self,
        node_id: &str,
        reports: &[SourceReport],
    ) -> Result<(), StoreError> {
        if reports.is_empty() {
            return Ok(());
        }

        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                r#"
                INSERT INTO forecast_snapshots (
                    node_id, source_id, issued_at, valid_at, lead_hours,
                    temp_c, feels_like_c, feels_like_is_derived,
                    rain_prob_pct, rain_mm, wind_kmh, humidity_pct, summary
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                ON CONFLICT(node_id, source_id, issued_at, valid_at) DO UPDATE SET
                    lead_hours = excluded.lead_hours,
                    temp_c = excluded.temp_c,
                    feels_like_c = excluded.feels_like_c,
                    feels_like_is_derived = excluded.feels_like_is_derived,
                    rain_prob_pct = excluded.rain_prob_pct,
                    rain_mm = excluded.rain_mm,
                    wind_kmh = excluded.wind_kmh,
                    humidity_pct = excluded.humidity_pct,
                    summary = excluded.summary
                "#,
            )?;

            for report in reports {
                let issued_str = format_ts(report.fetched_at);
                for point in report.hourly.iter().chain(std::iter::once(&report.current)) {
                    insert_forecast_point(
                        &mut stmt,
                        node_id,
                        &report.source_id,
                        report.fetched_at,
                        &issued_str,
                        point,
                    )?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Idempotently persists a node's aggregate `BestEstimate` (current + hourly) into
    /// `aggregate_snapshots` and caches the serialized [`NodeWeatherBundle`] in `node_bundles`.
    pub fn upsert_aggregate_bundle(
        &mut self,
        bundle: &NodeWeatherBundle,
        weights_version: &str,
    ) -> Result<(), StoreError> {
        let issued_at = bundle.generated_at;
        let issued_str = format_ts(issued_at);
        let bundle_json = serde_json::to_string(bundle)?;
        let node_id = &bundle.node.node_id;

        let tx = self.conn.transaction()?;
        {
            let mut node_stmt = tx.prepare(
                r#"
                INSERT INTO nodes (
                    node_id, name, state, lat, lon,
                    bom_station_id, bom_product_id, bom_forecast_product, is_major_hub
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                ON CONFLICT(node_id) DO UPDATE SET
                    name = excluded.name,
                    state = excluded.state,
                    lat = excluded.lat,
                    lon = excluded.lon,
                    bom_station_id = excluded.bom_station_id,
                    bom_product_id = excluded.bom_product_id,
                    bom_forecast_product = excluded.bom_forecast_product,
                    is_major_hub = excluded.is_major_hub
                "#,
            )?;
            upsert_node_with_stmt(&mut node_stmt, &bundle.node)?;

            let mut agg_stmt = tx.prepare(
                r#"
                INSERT INTO aggregate_snapshots (
                    node_id, issued_at, valid_at, lead_hours,
                    feels_like_c, temp_c, rain_verdict, rain_prob_pct,
                    rain_mm_expected, source_count, weights_version
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                ON CONFLICT(node_id, issued_at, valid_at) DO UPDATE SET
                    lead_hours = excluded.lead_hours,
                    feels_like_c = excluded.feels_like_c,
                    temp_c = excluded.temp_c,
                    rain_verdict = excluded.rain_verdict,
                    rain_prob_pct = excluded.rain_prob_pct,
                    rain_mm_expected = excluded.rain_mm_expected,
                    source_count = excluded.source_count,
                    weights_version = excluded.weights_version
                "#,
            )?;

            let source_count = i64::try_from(bundle.current_best_estimate.source_count)
                .map_err(|_| StoreError::InvalidData("source_count overflow".to_string()))?;

            // Insert hourly estimates first so current_best_estimate's exact RainVerdict wins on
            // matching `valid_at`.
            for hourly in &bundle.hourly_best_estimate {
                let valid_str = format_ts(hourly.valid_at);
                let lead_hours = compute_lead_hours(issued_at, hourly.valid_at);
                let verdict = derive_rain_verdict(hourly.rain_prob_pct, hourly.rain_mm_expected);
                agg_stmt.execute(params![
                    node_id,
                    &issued_str,
                    valid_str,
                    lead_hours,
                    hourly.feels_like_c,
                    hourly.temp_c,
                    rain_verdict_to_str(verdict),
                    hourly.rain_prob_pct,
                    hourly.rain_mm_expected,
                    source_count,
                    weights_version,
                ])?;
            }

            let cur = &bundle.current_best_estimate;
            agg_stmt.execute(params![
                node_id,
                &issued_str,
                format_ts(cur.valid_at),
                compute_lead_hours(issued_at, cur.valid_at),
                cur.feels_like_c,
                cur.temp_c,
                rain_verdict_to_str(cur.rain.verdict),
                cur.rain.probability_pct,
                cur.rain.expected_mm,
                source_count,
                weights_version,
            ])?;

            tx.execute(
                r#"
                INSERT INTO node_bundles (node_id, generated_at, bundle_json)
                VALUES (?1, ?2, ?3)
                ON CONFLICT(node_id) DO UPDATE SET
                    generated_at = excluded.generated_at,
                    bundle_json = excluded.bundle_json
                WHERE excluded.generated_at >= node_bundles.generated_at
                "#,
                params![node_id, &issued_str, bundle_json],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Idempotently inserts or updates ground-truth observation records.
    pub fn upsert_observations(&mut self, obs: &[ObservationRecord]) -> Result<(), StoreError> {
        if obs.is_empty() {
            return Ok(());
        }

        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                r#"
                INSERT INTO observations (
                    node_id, observed_at, source_id, quality_rank,
                    temp_c, feels_like_c, rain_mm_1h, rained_bool, wind_kmh, humidity_pct
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(node_id, observed_at, source_id) DO UPDATE SET
                    quality_rank = excluded.quality_rank,
                    temp_c = excluded.temp_c,
                    feels_like_c = excluded.feels_like_c,
                    rain_mm_1h = excluded.rain_mm_1h,
                    rained_bool = excluded.rained_bool,
                    wind_kmh = excluded.wind_kmh,
                    humidity_pct = excluded.humidity_pct
                "#,
            )?;

            for record in obs {
                let rained_int = record.rained_bool.map(i64::from);
                stmt.execute(params![
                    record.node_id,
                    format_ts(record.observed_at),
                    record.source_id,
                    i64::from(record.quality_rank),
                    record.temp_c,
                    record.feels_like_c,
                    record.rain_mm_1h,
                    rained_int,
                    record.wind_kmh,
                    record.humidity_pct,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Deletes historical forecast snapshots, aggregate snapshots, and observations strictly older
    /// than `cutoff`, preserving active future forecasts and rows at or after `cutoff`.
    pub fn prune_older_than(&mut self, cutoff: DateTime<Utc>) -> Result<PruneStats, StoreError> {
        let cutoff_str = format_ts(cutoff);
        let tx = self.conn.transaction()?;
        let forecast_snapshots_deleted = tx.execute(
            "DELETE FROM forecast_snapshots WHERE valid_at < ?1",
            params![&cutoff_str],
        )?;
        let aggregate_snapshots_deleted = tx.execute(
            "DELETE FROM aggregate_snapshots WHERE valid_at < ?1",
            params![&cutoff_str],
        )?;
        let observations_deleted = tx.execute(
            "DELETE FROM observations WHERE observed_at < ?1",
            params![&cutoff_str],
        )?;
        tx.commit()?;

        let total_deleted =
            forecast_snapshots_deleted + aggregate_snapshots_deleted + observations_deleted;
        if total_deleted > 0 {
            if let Err(err) = self
                .conn
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA incremental_vacuum(64);")
            {
                debug!(error = %err, "non-fatal wal_checkpoint/vacuum skipped");
            }
        }

        Ok(PruneStats {
            forecast_snapshots_deleted,
            aggregate_snapshots_deleted,
            observations_deleted,
        })
    }

    /// Fetches the latest cached [`NodeWeatherBundle`] for `node_id`.
    pub fn fetch_latest_node_bundle(
        &self,
        node_id: &str,
    ) -> Result<Option<NodeWeatherBundle>, StoreError> {
        let bundle_json: Option<String> = self
            .conn
            .query_row(
                "SELECT bundle_json FROM node_bundles WHERE node_id = ?1",
                params![node_id],
                |row| row.get(0),
            )
            .optional()?;

        match bundle_json {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }

    /// Lists all latest cached [`NodeWeatherBundle`] entries ordered by `node_id`.
    pub fn list_latest_node_bundles(&self) -> Result<Vec<NodeWeatherBundle>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT bundle_json FROM node_bundles ORDER BY node_id ASC")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut bundles = Vec::new();
        for row in rows {
            bundles.push(serde_json::from_str(&row?)?);
        }
        Ok(bundles)
    }

    /// Returns historical `forecast_snapshots` and `aggregate_snapshots` within `[since, until]`
    /// (inclusive) for regression scoring.
    pub fn list_historical_forecasts_for_regression(
        &self,
        since: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> Result<Vec<StoredForecastRow>, StoreError> {
        let since_str = format_ts(since);
        let until_str = format_ts(until);

        let mut stmt = self.conn.prepare(
            r#"
            SELECT node_id, source_id AS subject_id, 0 AS is_aggregate,
                   issued_at, valid_at, lead_hours,
                   temp_c, feels_like_c, rain_prob_pct, rain_mm
            FROM forecast_snapshots
            WHERE valid_at >= ?1 AND valid_at <= ?2

            UNION ALL

            SELECT node_id, ?3 AS subject_id, 1 AS is_aggregate,
                   issued_at, valid_at, lead_hours,
                   temp_c, feels_like_c, rain_prob_pct, rain_mm_expected AS rain_mm
            FROM aggregate_snapshots
            WHERE valid_at >= ?1 AND valid_at <= ?2

            ORDER BY valid_at ASC, node_id ASC, subject_id ASC, issued_at ASC
            "#,
        )?;

        let rows = stmt.query_map(params![since_str, until_str, AGGREGATE_SUBJECT_ID], |row| {
            let is_agg: i64 = row.get(2)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                is_agg != 0,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<f64>>(6)?,
                row.get::<_, Option<f64>>(7)?,
                row.get::<_, Option<f64>>(8)?,
                row.get::<_, Option<f64>>(9)?,
            ))
        })?;

        let mut results = Vec::new();
        for row in rows {
            let (
                node_id,
                subject_id,
                is_aggregate,
                issued_raw,
                valid_raw,
                lead_hours,
                temp_c,
                feels_like_c,
                rain_prob_pct,
                rain_mm,
            ) = row?;

            results.push(StoredForecastRow {
                node_id,
                subject_id,
                is_aggregate,
                issued_at: parse_ts(&issued_raw)?,
                valid_at: parse_ts(&valid_raw)?,
                lead_hours,
                temp_c,
                feels_like_c,
                rain_prob_pct,
                rain_mm,
            });
        }
        Ok(results)
    }

    /// Returns observations within `[since, until]` (inclusive), ordered chronologically and by
    /// `quality_rank` ascending (Tier 1 first).
    pub fn list_observations_window(
        &self,
        since: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> Result<Vec<ObservationRecord>, StoreError> {
        let since_str = format_ts(since);
        let until_str = format_ts(until);

        let mut stmt = self.conn.prepare(
            r#"
            SELECT node_id, observed_at, source_id, quality_rank,
                   temp_c, feels_like_c, rain_mm_1h, rained_bool, wind_kmh, humidity_pct
            FROM observations
            WHERE observed_at >= ?1 AND observed_at <= ?2
            ORDER BY observed_at ASC, node_id ASC, quality_rank ASC, source_id ASC
            "#,
        )?;

        let rows = stmt.query_map(params![since_str, until_str], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<f64>>(4)?,
                row.get::<_, Option<f64>>(5)?,
                row.get::<_, Option<f64>>(6)?,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, Option<f64>>(8)?,
                row.get::<_, Option<f64>>(9)?,
            ))
        })?;

        let mut records = Vec::new();
        for row in rows {
            let (
                node_id,
                observed_raw,
                source_id,
                quality_rank_raw,
                temp_c,
                feels_like_c,
                rain_mm_1h,
                rained_int,
                wind_kmh,
                humidity_pct,
            ) = row?;

            let quality_rank = u8::try_from(quality_rank_raw).map_err(|_| {
                StoreError::InvalidData(format!("invalid quality_rank {quality_rank_raw}"))
            })?;

            records.push(ObservationRecord {
                node_id,
                observed_at: parse_ts(&observed_raw)?,
                source_id,
                quality_rank,
                temp_c,
                feels_like_c,
                rain_mm_1h,
                rained_bool: rained_int.map(|v| v != 0),
                wind_kmh,
                humidity_pct,
            });
        }
        Ok(records)
    }

    /// Atomically replaces the materialized regression metrics and summary metadata.
    pub fn replace_regression_metrics(
        &mut self,
        summary: &RegressionSummary,
    ) -> Result<(), StoreError> {
        let weights_json = serde_json::to_string(&summary.suggested_weights)?;
        let summary_computed_at = format_ts(summary.computed_at);

        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM regression_metrics", [])?;

        {
            let mut stmt = tx.prepare(
                r#"
                INSERT INTO regression_metrics (
                    computed_at, window_days, subject_id, display_name, lead_bucket,
                    sample_count, temp_mae_c, temp_rmse_c, temp_bias_c,
                    feels_like_mae_c, feels_like_rmse_c, rain_brier_score, rain_hit_rate
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                "#,
            )?;

            for metric in &summary.metrics {
                let sample_count = i64::try_from(metric.sample_count)
                    .map_err(|_| StoreError::InvalidData("sample_count overflow".to_string()))?;
                stmt.execute(params![
                    format_ts(metric.computed_at),
                    i64::from(metric.window_days),
                    metric.subject_id,
                    metric.display_name,
                    metric.lead_bucket,
                    sample_count,
                    metric.temp_mae_c,
                    metric.temp_rmse_c,
                    metric.temp_bias_c,
                    metric.feels_like_mae_c,
                    metric.feels_like_rmse_c,
                    metric.rain_brier_score,
                    metric.rain_hit_rate,
                ])?;
            }
        }

        tx.execute(
            r#"
            INSERT INTO regression_meta (id, computed_at, window_days, suggested_weights_json)
            VALUES (1, ?1, ?2, ?3)
            ON CONFLICT(id) DO UPDATE SET
                computed_at = excluded.computed_at,
                window_days = excluded.window_days,
                suggested_weights_json = excluded.suggested_weights_json
            "#,
            params![
                summary_computed_at,
                i64::from(summary.window_days),
                weights_json
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Loads the latest materialized [`RegressionSummary`], or `None` if no metrics exist yet.
    pub fn load_regression_summary(&self) -> Result<Option<RegressionSummary>, StoreError> {
        let meta: Option<(String, i64, String)> = self
            .conn
            .query_row(
                "SELECT computed_at, window_days, suggested_weights_json FROM regression_meta WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;

        let mut stmt = self.conn.prepare(
            r#"
            SELECT computed_at, window_days, subject_id, display_name, lead_bucket,
                   sample_count, temp_mae_c, temp_rmse_c, temp_bias_c,
                   feels_like_mae_c, feels_like_rmse_c, rain_brier_score, rain_hit_rate
            FROM regression_metrics
            ORDER BY subject_id ASC, lead_bucket ASC
            "#,
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<f64>>(6)?,
                row.get::<_, Option<f64>>(7)?,
                row.get::<_, Option<f64>>(8)?,
                row.get::<_, Option<f64>>(9)?,
                row.get::<_, Option<f64>>(10)?,
                row.get::<_, Option<f64>>(11)?,
                row.get::<_, Option<f64>>(12)?,
            ))
        })?;

        let mut metrics = Vec::new();
        for row in rows {
            let (
                computed_raw,
                window_days_raw,
                subject_id,
                display_name,
                lead_bucket,
                sample_count_raw,
                temp_mae_c,
                temp_rmse_c,
                temp_bias_c,
                feels_like_mae_c,
                feels_like_rmse_c,
                rain_brier_score,
                rain_hit_rate,
            ) = row?;

            let window_days = u32::try_from(window_days_raw).map_err(|_| {
                StoreError::InvalidData(format!("invalid window_days {window_days_raw}"))
            })?;
            let sample_count = usize::try_from(sample_count_raw).map_err(|_| {
                StoreError::InvalidData(format!("invalid sample_count {sample_count_raw}"))
            })?;

            metrics.push(RegressionMetricRow {
                computed_at: parse_ts(&computed_raw)?,
                window_days,
                subject_id,
                display_name,
                lead_bucket,
                sample_count,
                temp_mae_c,
                temp_rmse_c,
                temp_bias_c,
                feels_like_mae_c,
                feels_like_rmse_c,
                rain_brier_score,
                rain_hit_rate,
            });
        }

        match meta {
            Some((computed_raw, window_days_raw, weights_json)) => {
                let window_days = u32::try_from(window_days_raw).map_err(|_| {
                    StoreError::InvalidData(format!("invalid window_days {window_days_raw}"))
                })?;
                let suggested_weights: HashMap<String, f64> = serde_json::from_str(&weights_json)?;
                Ok(Some(RegressionSummary {
                    computed_at: parse_ts(&computed_raw)?,
                    window_days,
                    metrics,
                    suggested_weights,
                }))
            }
            None if metrics.is_empty() => Ok(None),
            None => Ok(Some(RegressionSummary {
                computed_at: metrics[0].computed_at,
                window_days: metrics[0].window_days,
                metrics,
                suggested_weights: HashMap::new(),
            })),
        }
    }
}

fn upsert_node_with_stmt(
    stmt: &mut rusqlite::Statement<'_>,
    node: &MeshNode,
) -> Result<(), StoreError> {
    stmt.execute(params![
        node.node_id,
        node.name,
        state_code_to_str(&node.state),
        node.lat,
        node.lon,
        node.bom_station_id,
        node.bom_product_id,
        node.bom_forecast_product,
        i64::from(node.is_major_hub),
    ])?;
    Ok(())
}

fn insert_forecast_point(
    stmt: &mut rusqlite::Statement<'_>,
    node_id: &str,
    source_id: &str,
    issued_at: DateTime<Utc>,
    issued_str: &str,
    point: &WeatherPoint,
) -> Result<(), StoreError> {
    stmt.execute(params![
        node_id,
        source_id,
        issued_str,
        format_ts(point.valid_at),
        compute_lead_hours(issued_at, point.valid_at),
        point.temp_c,
        point.feels_like_c,
        i64::from(point.feels_like_is_derived),
        point.rain_prob_pct,
        point.rain_mm,
        point.wind_kmh,
        point.humidity_pct,
        point.summary,
    ])?;
    Ok(())
}

/// Formats timestamps as fixed-width nanosecond UTC RFC3339 strings so SQLite
/// lexicographical comparisons match chronological order.
fn format_ts(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn parse_ts(raw: &str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|err| StoreError::InvalidTimestamp(raw.to_string(), err))
}

fn compute_lead_hours(issued_at: DateTime<Utc>, valid_at: DateTime<Utc>) -> i64 {
    let secs = (valid_at - issued_at).num_seconds() as f64;
    (secs / 3600.0).round() as i64
}

fn state_code_to_str(state: &StateCode) -> &'static str {
    match state {
        StateCode::NSW => "NSW",
        StateCode::VIC => "VIC",
        StateCode::QLD => "QLD",
        StateCode::SA => "SA",
        StateCode::WA => "WA",
        StateCode::TAS => "TAS",
        StateCode::NT => "NT",
        StateCode::ACT => "ACT",
    }
}

fn parse_state_code(raw: &str) -> Result<StateCode, StoreError> {
    match raw {
        "NSW" => Ok(StateCode::NSW),
        "VIC" => Ok(StateCode::VIC),
        "QLD" => Ok(StateCode::QLD),
        "SA" => Ok(StateCode::SA),
        "WA" => Ok(StateCode::WA),
        "TAS" => Ok(StateCode::TAS),
        "NT" => Ok(StateCode::NT),
        "ACT" => Ok(StateCode::ACT),
        other => Err(StoreError::InvalidStateCode(other.to_string())),
    }
}

fn rain_verdict_to_str(verdict: RainVerdict) -> &'static str {
    match verdict {
        RainVerdict::Dry => "dry",
        RainVerdict::Possible => "possible",
        RainVerdict::Likely => "likely",
        RainVerdict::Raining => "raining",
    }
}

fn derive_rain_verdict(rain_prob_pct: f64, rain_mm_expected: f64) -> RainVerdict {
    if rain_mm_expected >= 0.2 {
        RainVerdict::Raining
    } else if rain_prob_pct >= 60.0 {
        RainVerdict::Likely
    } else if rain_prob_pct >= 25.0 {
        RainVerdict::Possible
    } else {
        RainVerdict::Dry
    }
}
