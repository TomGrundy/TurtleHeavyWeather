use std::sync::Arc;

use chrono::Utc;
use tempfile::tempdir;
use thw_collector::run_sync_cycle_with_sources;
use thw_core::{AggregationConfig, MeshNode, NodeWeatherBundle, RegressionSummary, StateCode};
use thw_sources::bom::BomObsJsonSource;
use thw_sources::open_meteo::{OpenMeteoModel, OpenMeteoSource};
use thw_sources::WeatherSource;
use thw_store::WeatherStore;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sample_node() -> MeshNode {
    MeshNode {
        node_id: "bom-066062".into(),
        name: "Sydney (Observatory Hill)".into(),
        state: StateCode::NSW,
        lat: -33.8607,
        lon: 151.205,
        bom_station_id: Some("94768".into()),
        bom_product_id: Some("IDN60901".into()),
        bom_forecast_product: Some("IDN11060".into()),
        is_major_hub: true,
    }
}

#[tokio::test]
async fn collector_runs_full_sync_regress_prune_and_static_export() {
    let server = MockServer::start().await;
    let now_utc = Utc::now().format("%Y%m%d%H0000").to_string();

    Mock::given(method("GET"))
        .and(path("/fwo/IDN60901/IDN60901.94768.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "observations": {
                "data": [{
                    "aifstime_utc": now_utc,
                    "air_temp": 22.5,
                    "apparent_t": 21.8,
                    "rain_trace": "0.0",
                    "rel_hum": 60.0,
                    "wind_spd_kmh": 15.0,
                    "weather": "Fine"
                }]
            }
        })))
        .mount(&server)
        .await;

    let hour_iso = Utc::now().format("%Y-%m-%dT%H:00").to_string();
    Mock::given(method("GET"))
        .and(path("/v1/forecast"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "latitude": -33.8607,
            "longitude": 151.205,
            "hourly": {
                "time": [hour_iso],
                "temperature_2m": [23.1],
                "apparent_temperature": [22.4],
                "precipitation_probability": [30.0],
                "precipitation": [0.2],
                "wind_speed_10m": [16.0],
                "relative_humidity_2m": [62.0]
            }
        })))
        .mount(&server)
        .await;

    let dir = tempdir().unwrap();
    let db_path = dir.path().join("weather.db");
    let export_dir = dir.path().join("export");
    let mut store = WeatherStore::open(&db_path).unwrap();

    let sources: Vec<Arc<dyn WeatherSource>> = vec![
        Arc::new(BomObsJsonSource::with_base_url(server.uri())),
        Arc::new(OpenMeteoSource::new(OpenMeteoModel::BomAccessGlobal).with_base_url(server.uri())),
    ];

    let report = run_sync_cycle_with_sources(
        &mut store,
        &[sample_node()],
        &sources,
        &AggregationConfig::default(),
        Some(&export_dir),
        None,
        7,
        7,
    )
    .await
    .expect("collector cycle should succeed");

    assert_eq!(report.sources_attempted, 2);
    assert_eq!(report.sources_succeeded, 2);
    assert_eq!(report.bundles_generated, 1);

    let bundle_raw = std::fs::read_to_string(export_dir.join("nodes/bom-066062.json")).unwrap();
    let bundle: NodeWeatherBundle = serde_json::from_str(&bundle_raw).unwrap();
    assert_eq!(bundle.sources.len(), 2);

    let reg_raw = std::fs::read_to_string(export_dir.join("regression/summary.json")).unwrap();
    let _reg: RegressionSummary = serde_json::from_str(&reg_raw).unwrap();
}
