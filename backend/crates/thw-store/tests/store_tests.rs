use std::collections::HashMap;

use chrono::{DateTime, Duration, TimeZone, Utc};
use tempfile::tempdir;
use thw_core::{
    BestEstimate, ConfidenceLevel, HourlyEstimate, MeshNode, NodeWeatherBundle, ObservationRecord,
    RainSummary, RainVerdict, RegressionMetricRow, RegressionSummary, SourceKind, SourceReport,
    StateCode, TempSpread, WeatherPoint,
};
use thw_store::{migrations, WeatherStore, AGGREGATE_SUBJECT_ID};

fn ts(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, hour, min, sec)
        .unwrap()
}

fn sample_node(node_id: &str, name: &str, state: StateCode) -> MeshNode {
    MeshNode {
        node_id: node_id.to_string(),
        name: name.to_string(),
        state,
        lat: -33.8688,
        lon: 151.2093,
        bom_station_id: Some("066062".to_string()),
        bom_product_id: Some("IDN60901".to_string()),
        bom_forecast_product: Some("IDN10064".to_string()),
        is_major_hub: true,
    }
}

fn sample_bundle(node: MeshNode, generated_at: DateTime<Utc>) -> NodeWeatherBundle {
    NodeWeatherBundle {
        generated_at,
        node,
        current_best_estimate: BestEstimate {
            valid_at: generated_at,
            feels_like_c: 21.4,
            temp_c: 22.8,
            rain: RainSummary {
                verdict: RainVerdict::Possible,
                label: "Possible light showers".to_string(),
                probability_pct: 35.0,
                expected_mm: 0.1,
                is_raining_now: false,
            },
            confidence: ConfidenceLevel::High,
            spread: TempSpread {
                feels_like_min_c: 20.5,
                feels_like_max_c: 22.1,
                temp_min_c: 22.0,
                temp_max_c: 23.5,
            },
            source_count: 3,
        },
        hourly_best_estimate: vec![
            HourlyEstimate {
                valid_at: generated_at + Duration::hours(1),
                feels_like_c: 21.0,
                temp_c: 22.2,
                rain_prob_pct: 40.0,
                rain_mm_expected: 0.1,
            },
            HourlyEstimate {
                valid_at: generated_at + Duration::hours(6),
                feels_like_c: 18.5,
                temp_c: 19.3,
                rain_prob_pct: 70.0,
                rain_mm_expected: 1.4,
            },
        ],
        sources: vec![SourceReport {
            source_id: "open_meteo_bom_access".to_string(),
            display_name: "Open-Meteo BOM ACCESS".to_string(),
            kind: SourceKind::Forecast,
            fetched_at: generated_at,
            weight: 1.25,
            current: WeatherPoint {
                valid_at: generated_at,
                temp_c: Some(22.5),
                feels_like_c: Some(21.2),
                feels_like_is_derived: true,
                rain_prob_pct: Some(30.0),
                rain_mm: Some(0.0),
                wind_kmh: Some(15.0),
                humidity_pct: Some(62.0),
                summary: Some("Partly cloudy".to_string()),
            },
            hourly: vec![WeatherPoint {
                valid_at: generated_at + Duration::hours(1),
                temp_c: Some(22.0),
                feels_like_c: Some(20.9),
                feels_like_is_derived: true,
                rain_prob_pct: Some(40.0),
                rain_mm: Some(0.1),
                wind_kmh: Some(16.0),
                humidity_pct: Some(64.0),
                summary: None,
            }],
            source_url: Some("https://api.open-meteo.com/".to_string()),
        }],
    }
}

#[test]
fn migrations_initialize_all_tables_and_wal_mode() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("nested").join("weather.db");

    let mut store = WeatherStore::open(&db_path).unwrap();
    assert_eq!(store.schema_version().unwrap(), 1);

    let journal_mode: String = store
        .connection()
        .query_row("PRAGMA journal_mode;", [], |r| r.get(0))
        .unwrap();
    assert_eq!(journal_mode.to_lowercase(), "wal");

    let fk_enabled: i64 = store
        .connection()
        .query_row("PRAGMA foreign_keys;", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fk_enabled, 1);

    // Running migrations again is a no-op.
    migrations::run_migrations(&mut rusqlite::Connection::open(&db_path).unwrap()).unwrap();

    for table in [
        "nodes",
        "forecast_snapshots",
        "aggregate_snapshots",
        "observations",
        "regression_metrics",
        "regression_meta",
        "node_bundles",
        "schema_migrations",
    ] {
        let exists: bool = store
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert!(exists, "expected table {table} to exist");
    }

    // Reopen read-only and verify reads work.
    let ro = WeatherStore::open_read_only(&db_path).unwrap();
    assert!(ro.list_nodes().unwrap().is_empty());
    // Satisfy mut binding lint
    store.upsert_nodes(&[]).unwrap();
}

#[test]
fn node_and_source_report_upserts_are_idempotent() {
    let mut store = WeatherStore::open_in_memory().unwrap();
    let mut node = sample_node("bom-066062", "Sydney Observatory Hill", StateCode::NSW);
    store.upsert_nodes(std::slice::from_ref(&node)).unwrap();

    node.name = "Sydney Observatory Hill AWS".to_string();
    node.is_major_hub = false;
    store.upsert_nodes(std::slice::from_ref(&node)).unwrap();

    let listed = store.list_nodes().unwrap();
    assert_eq!(listed, vec![node.clone()]);

    let issued = ts(2026, 11, 25, 0, 0, 0);
    let mut report = SourceReport {
        source_id: "met_no".to_string(),
        display_name: "MET Norway".to_string(),
        kind: SourceKind::Forecast,
        fetched_at: issued,
        weight: 1.0,
        current: WeatherPoint {
            valid_at: issued,
            temp_c: Some(21.0),
            feels_like_c: Some(20.0),
            feels_like_is_derived: true,
            rain_prob_pct: Some(20.0),
            rain_mm: Some(0.0),
            wind_kmh: Some(14.0),
            humidity_pct: Some(60.0),
            summary: Some("Fair".to_string()),
        },
        hourly: vec![
            WeatherPoint {
                valid_at: issued + Duration::hours(1),
                temp_c: Some(21.5),
                feels_like_c: Some(20.5),
                feels_like_is_derived: true,
                rain_prob_pct: Some(25.0),
                rain_mm: Some(0.1),
                wind_kmh: Some(15.0),
                humidity_pct: Some(61.0),
                summary: None,
            },
            WeatherPoint {
                valid_at: issued + Duration::hours(6),
                temp_c: Some(19.0),
                feels_like_c: Some(18.0),
                feels_like_is_derived: false,
                rain_prob_pct: Some(65.0),
                rain_mm: Some(1.2),
                wind_kmh: Some(18.0),
                humidity_pct: Some(75.0),
                summary: Some("Showers".to_string()),
            },
        ],
        source_url: None,
    };

    store
        .upsert_source_reports(&node.node_id, &[report.clone()])
        .unwrap();

    // Update a value and re-upsert the exact same (node_id, source_id, issued_at, valid_at) key.
    report.current.temp_c = Some(21.8);
    store
        .upsert_source_reports(&node.node_id, &[report])
        .unwrap();

    let count: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM forecast_snapshots", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 3);

    let rows = store
        .list_historical_forecasts_for_regression(issued, issued + Duration::hours(10))
        .unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].temp_c, Some(21.8));
    assert_eq!(rows[0].lead_hours, 0);
    assert_eq!(rows[1].lead_hours, 1);
    assert_eq!(rows[2].lead_hours, 6);
}

#[test]
fn aggregate_bundle_persists_snapshots_and_roundtrips_latest_bundle() {
    let mut store = WeatherStore::open_in_memory().unwrap();
    let node = sample_node("hub-melbourne", "Melbourne CBD", StateCode::VIC);

    let older_time = ts(2026, 11, 24, 22, 0, 0);
    let newer_time = ts(2026, 11, 24, 23, 0, 0);

    let older_bundle = sample_bundle(node.clone(), older_time);
    let newer_bundle = sample_bundle(node.clone(), newer_time);

    store
        .upsert_aggregate_bundle(&newer_bundle, "v1-static")
        .unwrap();
    // Backfilling an older bundle must not overwrite the newer `node_bundles` cache entry.
    store
        .upsert_aggregate_bundle(&older_bundle, "v1-static")
        .unwrap();

    let fetched = store
        .fetch_latest_node_bundle(&node.node_id)
        .unwrap()
        .expect("bundle should exist");
    assert_eq!(fetched, newer_bundle);
    assert!(store
        .fetch_latest_node_bundle("non-existent")
        .unwrap()
        .is_none());
    assert_eq!(
        store.list_latest_node_bundles().unwrap(),
        vec![newer_bundle]
    );

    // Both bundles' snapshots (3 rows each: current + 2 hourly) are kept in aggregate_snapshots.
    let agg_count: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM aggregate_snapshots", [], |r| r.get(0))
        .unwrap();
    assert_eq!(agg_count, 6);

    let regression_rows = store
        .list_historical_forecasts_for_regression(older_time, newer_time + Duration::hours(12))
        .unwrap();
    assert_eq!(regression_rows.len(), 6);
    assert!(regression_rows
        .iter()
        .all(|r| r.is_aggregate && r.subject_id == AGGREGATE_SUBJECT_ID));
}

#[test]
fn observation_upserts_and_retention_pruning_boundaries() {
    let mut store = WeatherStore::open_in_memory().unwrap();
    let node = sample_node("bom-094029", "Hobart Ellerslie Road", StateCode::TAS);
    store.upsert_nodes(std::slice::from_ref(&node)).unwrap();

    let now = ts(2026, 11, 25, 12, 0, 0);
    let cutoff = now - Duration::days(7); // 2026-11-18T12:00:00Z
    let expired = cutoff - Duration::seconds(1);
    let exact_boundary = cutoff;
    let recent = now - Duration::hours(3);
    let future = now + Duration::hours(24);

    // Insert source forecasts across all 4 boundary timestamps.
    let report = SourceReport {
        source_id: "bom_ftp".to_string(),
        display_name: "BOM FTP".to_string(),
        kind: SourceKind::Forecast,
        fetched_at: cutoff - Duration::hours(6),
        weight: 1.5,
        current: WeatherPoint {
            valid_at: expired,
            temp_c: Some(12.0),
            feels_like_c: Some(10.0),
            feels_like_is_derived: false,
            rain_prob_pct: Some(10.0),
            rain_mm: Some(0.0),
            wind_kmh: Some(20.0),
            humidity_pct: Some(70.0),
            summary: None,
        },
        hourly: vec![
            WeatherPoint {
                valid_at: exact_boundary,
                temp_c: Some(13.0),
                feels_like_c: Some(11.0),
                feels_like_is_derived: false,
                rain_prob_pct: Some(15.0),
                rain_mm: Some(0.0),
                wind_kmh: Some(18.0),
                humidity_pct: Some(68.0),
                summary: None,
            },
            WeatherPoint {
                valid_at: recent,
                temp_c: Some(16.0),
                feels_like_c: Some(15.0),
                feels_like_is_derived: false,
                rain_prob_pct: Some(50.0),
                rain_mm: Some(0.4),
                wind_kmh: Some(12.0),
                humidity_pct: Some(65.0),
                summary: None,
            },
            WeatherPoint {
                valid_at: future,
                temp_c: Some(18.0),
                feels_like_c: Some(17.5),
                feels_like_is_derived: false,
                rain_prob_pct: Some(5.0),
                rain_mm: Some(0.0),
                wind_kmh: Some(10.0),
                humidity_pct: Some(55.0),
                summary: None,
            },
        ],
        source_url: None,
    };
    store.upsert_source_reports(&node.node_id, &[report]).unwrap();

    // Insert aggregate bundle with an expired current estimate and a recent hourly estimate.
    let mut bundle = sample_bundle(node.clone(), expired);
    bundle.current_best_estimate.valid_at = expired;
    bundle.hourly_best_estimate = vec![
        HourlyEstimate {
            valid_at: exact_boundary,
            feels_like_c: 11.2,
            temp_c: 13.1,
            rain_prob_pct: 20.0,
            rain_mm_expected: 0.0,
        },
        HourlyEstimate {
            valid_at: future,
            feels_like_c: 17.0,
            temp_c: 18.2,
            rain_prob_pct: 10.0,
            rain_mm_expected: 0.0,
        },
    ];
    store.upsert_aggregate_bundle(&bundle, "v1-static").unwrap();

    // Insert observations (including duplicate key update and multiple quality ranks).
    let obs = vec![
        ObservationRecord {
            node_id: node.node_id.clone(),
            observed_at: expired,
            source_id: "bom_obs_json".to_string(),
            quality_rank: 1,
            temp_c: Some(12.1),
            feels_like_c: Some(10.2),
            rain_mm_1h: Some(0.0),
            rained_bool: Some(false),
            wind_kmh: Some(19.0),
            humidity_pct: Some(71.0),
        },
        ObservationRecord {
            node_id: node.node_id.clone(),
            observed_at: exact_boundary,
            source_id: "bom_obs_json".to_string(),
            quality_rank: 1,
            temp_c: Some(13.2),
            feels_like_c: Some(11.3),
            rain_mm_1h: Some(0.0),
            rained_bool: Some(false),
            wind_kmh: Some(17.0),
            humidity_pct: Some(68.0),
        },
        ObservationRecord {
            node_id: node.node_id.clone(),
            observed_at: recent,
            source_id: "open_meteo_reanalysis".to_string(),
            quality_rank: 2,
            temp_c: Some(15.5),
            feels_like_c: Some(14.5),
            rain_mm_1h: Some(0.5),
            rained_bool: Some(true),
            wind_kmh: Some(12.0),
            humidity_pct: Some(66.0),
        },
        ObservationRecord {
            node_id: node.node_id.clone(),
            observed_at: recent,
            source_id: "bom_obs_json".to_string(),
            quality_rank: 1,
            temp_c: Some(15.8),
            feels_like_c: Some(14.9),
            rain_mm_1h: Some(0.6),
            rained_bool: Some(true),
            wind_kmh: Some(11.0),
            humidity_pct: Some(65.0),
        },
    ];
    store.upsert_observations(&obs).unwrap();

    // Prune strictly older than `cutoff`.
    let stats = store.prune_older_than(cutoff).unwrap();
    assert_eq!(stats.forecast_snapshots_deleted, 1);
    assert_eq!(stats.aggregate_snapshots_deleted, 1);
    assert_eq!(stats.observations_deleted, 1);

    // Remaining observations: exact_boundary + recent (Tier 1 before Tier 2).
    let remaining_obs = store
        .list_observations_window(expired, future)
        .unwrap();
    assert_eq!(remaining_obs.len(), 3);
    assert_eq!(remaining_obs[0].observed_at, exact_boundary);
    assert_eq!(remaining_obs[1].observed_at, recent);
    assert_eq!(remaining_obs[1].quality_rank, 1);
    assert_eq!(remaining_obs[2].quality_rank, 2);

    // Remaining forecasts + aggregates: 3 source + 2 aggregate = 5 (expired rows removed, future kept).
    let remaining_forecasts = store
        .list_historical_forecasts_for_regression(expired, future)
        .unwrap();
    assert_eq!(remaining_forecasts.len(), 5);
    assert!(remaining_forecasts.iter().all(|r| r.valid_at >= cutoff));
    assert!(remaining_forecasts.iter().any(|r| r.valid_at == future));

    // Latest node bundle cache remains intact.
    assert!(store
        .fetch_latest_node_bundle(&node.node_id)
        .unwrap()
        .is_some());
}

#[test]
fn regression_metrics_round_trip_and_replace() {
    let mut store = WeatherStore::open_in_memory().unwrap();
    assert!(store.load_regression_summary().unwrap().is_none());

    let computed_at = ts(2026, 11, 25, 1, 0, 0);
    let mut weights = HashMap::new();
    weights.insert("bom_ftp".to_string(), 1.45);
    weights.insert("met_no".to_string(), 1.05);

    let summary_v1 = RegressionSummary {
        computed_at,
        window_days: 7,
        metrics: vec![
            RegressionMetricRow {
                computed_at,
                window_days: 7,
                subject_id: AGGREGATE_SUBJECT_ID.to_string(),
                display_name: "TurtleHeavyWeather Best Estimate".to_string(),
                lead_bucket: "0-6h".to_string(),
                sample_count: 42,
                temp_mae_c: Some(0.8),
                temp_rmse_c: Some(1.1),
                temp_bias_c: Some(-0.1),
                feels_like_mae_c: Some(0.9),
                feels_like_rmse_c: Some(1.2),
                rain_brier_score: Some(0.08),
                rain_hit_rate: Some(0.91),
            },
            RegressionMetricRow {
                computed_at,
                window_days: 7,
                subject_id: "bom_ftp".to_string(),
                display_name: "BOM FTP".to_string(),
                lead_bucket: "0-6h".to_string(),
                sample_count: 40,
                temp_mae_c: Some(1.0),
                temp_rmse_c: Some(1.3),
                temp_bias_c: Some(0.2),
                feels_like_mae_c: Some(1.1),
                feels_like_rmse_c: Some(1.4),
                rain_brier_score: Some(0.11),
                rain_hit_rate: Some(0.87),
            },
        ],
        suggested_weights: weights,
    };

    store.replace_regression_metrics(&summary_v1).unwrap();
    let loaded_v1 = store.load_regression_summary().unwrap().unwrap();
    assert_eq!(loaded_v1, summary_v1);

    // Replace with a newer empty-weights summary and verify stale rows are removed.
    let computed_at_v2 = computed_at + Duration::hours(1);
    let summary_v2 = RegressionSummary {
        computed_at: computed_at_v2,
        window_days: 7,
        metrics: vec![RegressionMetricRow {
            computed_at: computed_at_v2,
            window_days: 7,
            subject_id: "met_no".to_string(),
            display_name: "MET Norway".to_string(),
            lead_bucket: "6-24h".to_string(),
            sample_count: 15,
            temp_mae_c: Some(1.4),
            temp_rmse_c: Some(1.7),
            temp_bias_c: Some(0.5),
            feels_like_mae_c: None,
            feels_like_rmse_c: None,
            rain_brier_score: Some(0.14),
            rain_hit_rate: Some(0.8),
        }],
        suggested_weights: HashMap::new(),
    };

    store.replace_regression_metrics(&summary_v2).unwrap();
    let loaded_v2 = store.load_regression_summary().unwrap().unwrap();
    assert_eq!(loaded_v2, summary_v2);
}
