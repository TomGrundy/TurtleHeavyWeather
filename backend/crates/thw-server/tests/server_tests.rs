use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Utc;
use tempfile::tempdir;
use thw_core::{
    aggregate_node_bundle, AggregationConfig, LocationIndexEntry, MeshNode, RegressionMetricRow,
    RegressionSummary, SourceKind, SourceReport, StateCode, WeatherPoint,
};
use thw_server::{build_router, AppState};
use thw_store::WeatherStore;
use tower::ServiceExt;

fn test_state() -> (tempfile::TempDir, AppState) {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("weather.db");
    let now = Utc::now();

    let node = MeshNode {
        node_id: "bom-066062".into(),
        name: "Sydney (Observatory Hill)".into(),
        state: StateCode::NSW,
        lat: -33.8607,
        lon: 151.205,
        bom_station_id: Some("94768".into()),
        bom_product_id: Some("IDN60901".into()),
        bom_forecast_product: Some("IDN11060".into()),
        is_major_hub: true,
    };

    let loc = LocationIndexEntry {
        suburb: "Sydney".into(),
        postcode: "2000".into(),
        state: StateCode::NSW,
        lat: -33.8688,
        lon: 151.2093,
        node_id: "bom-066062".into(),
        distance_km: 1.0,
    };

    let report = SourceReport {
        source_id: "bom_obs_json".into(),
        display_name: "BOM Obs".into(),
        kind: SourceKind::Observation,
        fetched_at: now,
        weight: 1.5,
        current: WeatherPoint {
            valid_at: now,
            temp_c: Some(24.0),
            feels_like_c: Some(23.1),
            feels_like_is_derived: false,
            rain_prob_pct: Some(10.0),
            rain_mm: Some(0.0),
            wind_kmh: Some(14.0),
            humidity_pct: Some(55.0),
            summary: Some("Sunny".into()),
        },
        hourly: vec![],
        source_url: None,
    };

    let bundle =
        aggregate_node_bundle(&node, &[report], now, &AggregationConfig::default()).unwrap();

    let mut store = WeatherStore::open(&db_path).unwrap();
    store.upsert_nodes(std::slice::from_ref(&node)).unwrap();
    store.upsert_aggregate_bundle(&bundle, "v1-static").unwrap();
    store
        .replace_regression_metrics(&RegressionSummary {
            computed_at: now,
            window_days: 7,
            metrics: vec![RegressionMetricRow {
                computed_at: now,
                window_days: 7,
                subject_id: "__aggregate__".into(),
                display_name: "Best Estimate".into(),
                lead_bucket: "0-6h".into(),
                sample_count: 10,
                temp_mae_c: Some(0.8),
                temp_rmse_c: Some(1.0),
                temp_bias_c: Some(0.1),
                feels_like_mae_c: Some(0.9),
                feels_like_rmse_c: Some(1.1),
                rain_brier_score: Some(0.08),
                rain_hit_rate: Some(0.9),
            }],
            suggested_weights: HashMap::new(),
        })
        .unwrap();

    let state = AppState {
        db_path,
        nodes: Arc::new(vec![node]),
        locations: Arc::new(vec![loc]),
    };
    (dir, state)
}

#[tokio::test]
async fn server_routes_return_expected_responses() {
    let (_dir, state) = test_state();
    let app = build_router(state, None);

    let health = app
        .clone()
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);

    let locs = app
        .clone()
        .oneshot(
            Request::get("/api/v1/locations?q=2000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(locs.status(), StatusCode::OK);

    let weather_ok = app
        .clone()
        .oneshot(
            Request::get("/api/v1/weather?lat=-33.87&lon=151.21")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(weather_ok.status(), StatusCode::OK);

    let weather_oob = app
        .clone()
        .oneshot(
            Request::get("/api/v1/weather?lat=51.5&lon=-0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(weather_oob.status(), StatusCode::BAD_REQUEST);

    let reg = app
        .oneshot(
            Request::get("/api/v1/regression")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(reg.status(), StatusCode::OK);
}
