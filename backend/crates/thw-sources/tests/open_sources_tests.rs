use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use serde_json::json;
use thw_core::{compute_apparent_temp_c, MeshNode, StateCode};
use thw_sources::met_no::MetNorwaySource;
use thw_sources::open_meteo::{OpenMeteoModel, OpenMeteoSource};
use thw_sources::{FetchContext, SourceError, WeatherSource};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sample_node(idx: usize) -> MeshNode {
    MeshNode {
        node_id: format!("node-{idx:03}"),
        name: format!("Test Node {idx}"),
        state: StateCode::NSW,
        lat: -33.87 + (idx as f64) * 0.01,
        lon: 151.21 + (idx as f64) * 0.01,
        bom_station_id: None,
        bom_product_id: None,
        bom_forecast_product: None,
        is_major_hub: idx == 0,
    }
}

#[tokio::test]
async fn open_meteo_batches_105_nodes_into_three_requests() {
    let server = MockServer::start().await;
    let nodes: Vec<MeshNode> = (0..105).map(sample_node).collect();
    let now_ts = Utc::now().format("%Y-%m-%dT%H:00").to_string();

    // Batch 1 (50), Batch 2 (50), Batch 3 (5)
    Mock::given(method("GET"))
        .and(path("/v1/forecast"))
        .and(query_param("models", "bom_access_global"))
        .and(query_param("timezone", "UTC"))
        .respond_with(move |req: &wiremock::Request| {
            let lats = req
                .url
                .query_pairs()
                .find(|(k, _)| k == "latitude")
                .map(|(_, v)| v.into_owned())
                .unwrap_or_default();
            let count = lats.split(',').filter(|s| !s.is_empty()).count();
            let items: Vec<_> = (0..count)
                .map(|i| {
                    json!({
                        "latitude": -33.87,
                        "longitude": 151.21,
                        "hourly": {
                            "time": [now_ts],
                            "temperature_2m": [20.0 + i as f64 * 0.1],
                            "apparent_temperature": [19.5],
                            "precipitation_probability": [15],
                            "precipitation": [0.0],
                            "wind_speed_10m": [12.0],
                            "relative_humidity_2m": [55]
                        }
                    })
                })
                .collect();
            ResponseTemplate::new(200).set_body_json(items)
        })
        .expect(3)
        .mount(&server)
        .await;

    let source = OpenMeteoSource::new(OpenMeteoModel::BomAccessGlobal)
        .with_base_url(server.uri())
        .with_batch_size(50);

    let output = source
        .fetch_mesh(&nodes, &FetchContext::default())
        .await
        .expect("batch fetch should succeed");

    assert_eq!(source.source_id(), "open_meteo_bom_access");
    assert_eq!(source.display_name(), "Open-Meteo (BOM ACCESS-G)");
    assert_eq!(output.reports.len(), 105);
    assert_eq!(output.reports[0].0, "node-000");
    assert_eq!(output.reports[0].1.source_id, "open_meteo_bom_access");
    assert_eq!(output.reports[0].1.current.feels_like_c, Some(19.5));
    assert!(!output.reports[0].1.current.feels_like_is_derived);
}

#[tokio::test]
async fn open_meteo_handles_single_object_response_and_missing_field_fallback() {
    let server = MockServer::start().await;
    let node = sample_node(0);
    let t0 = Utc::now().format("%Y-%m-%dT%H:00").to_string();
    let t1 = (Utc::now() + ChronoDuration::hours(1))
        .format("%Y-%m-%dT%H:00")
        .to_string();

    // Single coordinate returns a JSON object (not an array), and apparent_temperature is null/missing.
    Mock::given(method("GET"))
        .and(path("/v1/forecast"))
        .and(query_param("models", "ecmwf_ifs025"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "latitude": node.lat,
            "longitude": node.lon,
            "hourly": {
                "time": [t0, t1],
                "temperature_2m": [25.0, 26.0],
                "apparent_temperature": [null, 25.4],
                "precipitation_probability": [null, 40],
                "precipitation": [0.0, 0.8],
                "wind_speed_10m": [18.0, 14.0],
                "relative_humidity_2m": [60, 58]
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let source = OpenMeteoSource::new(OpenMeteoModel::EcmwfIfs)
        .with_base_url(server.uri())
        .with_tier2_observations(true);

    let output = source
        .fetch_mesh(&[node], &FetchContext::default())
        .await
        .expect("single object fetch should succeed");

    assert_eq!(source.source_id(), "open_meteo_ecmwf");
    assert_eq!(output.reports.len(), 1);
    let report = &output.reports[0].1;
    assert_eq!(report.hourly.len(), 2);

    // Hour 0 had null apparent_temperature -> derived from 25°C, 60% RH, 18 km/h.
    let expected_at = compute_apparent_temp_c(25.0, 60.0, 18.0);
    assert_eq!(report.hourly[0].feels_like_c, Some(expected_at));
    assert!(report.hourly[0].feels_like_is_derived);
    assert_eq!(report.hourly[0].rain_prob_pct, None);

    // Hour 1 kept native apparent_temperature.
    assert_eq!(report.hourly[1].feels_like_c, Some(25.4));
    assert!(!report.hourly[1].feels_like_is_derived);

    // Tier-2 observation emitted when enabled.
    assert_eq!(output.observations.len(), 1);
    assert_eq!(output.observations[0].quality_rank, 2);
    assert_eq!(output.observations[0].source_id, "open_meteo_ecmwf");
}

#[tokio::test]
async fn open_meteo_gfs_metadata_and_rate_limit_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/forecast"))
        .and(query_param("models", "gfs_global"))
        .respond_with(ResponseTemplate::new(429))
        .expect(1)
        .mount(&server)
        .await;

    let source = OpenMeteoSource::gfs().with_base_url(server.uri());
    assert_eq!(source.source_id(), "open_meteo_gfs");
    assert_eq!(source.display_name(), "Open-Meteo (NOAA GFS)");

    let err = source
        .fetch_mesh(&[sample_node(0)], &FetchContext::default())
        .await
        .expect_err("429 should return RateLimited");
    assert!(matches!(err, SourceError::RateLimited(ref id) if id == "open_meteo_gfs"));
}

#[tokio::test]
async fn met_no_enforces_user_agent_parses_compact_and_derives_feels_like() {
    let server = MockServer::start().await;
    let node = sample_node(0);
    let t0 = Utc::now().to_rfc3339();
    let t1 = (Utc::now() + ChronoDuration::hours(6)).to_rfc3339();
    let ua = "TurtleHeavyWeatherTest/0.1 (contact@example.org)";

    Mock::given(method("GET"))
        .and(path("/weatherapi/locationforecast/2.0/compact"))
        .and(query_param("lat", "-33.87"))
        .and(query_param("lon", "151.21"))
        .and(header("user-agent", ua))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "type": "Feature",
            "properties": {
                "timeseries": [
                    {
                        "time": t0,
                        "data": {
                            "instant": {
                                "details": {
                                    "air_temperature": 25.0,
                                    "relative_humidity": 60.0,
                                    "wind_speed": 5.0
                                }
                            },
                            "next_1_hours": {
                                "summary": { "symbol_code": "partlycloudy_day" },
                                "details": {
                                    "precipitation_amount": 0.4,
                                    "probability_of_precipitation": 30.0
                                }
                            }
                        }
                    },
                    {
                        "time": t1,
                        "data": {
                            "instant": {
                                "details": {
                                    "air_temperature": 21.0,
                                    "relative_humidity": 75.0,
                                    "wind_speed": 2.0
                                }
                            },
                            "next_6_hours": {
                                "summary": { "symbol_code": "lightrain" },
                                "details": {
                                    "precipitation_amount": 1.8
                                }
                            }
                        }
                    }
                ]
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let source = MetNorwaySource::new()
        .with_base_url(server.uri())
        .with_request_delay(Duration::ZERO);
    let ctx = FetchContext {
        user_agent: ua.to_string(),
        timeout_secs: 5,
    };

    let output = source
        .fetch_mesh(&[node], &ctx)
        .await
        .expect("MET Norway fetch should succeed");

    assert_eq!(source.source_id(), "met_no");
    assert_eq!(output.reports.len(), 1);
    let report = &output.reports[0].1;
    assert_eq!(report.hourly.len(), 2);

    // Hour 0: 5.0 m/s == 18.0 km/h; apparent temp derived natively via Steadman formula.
    let expected_at = compute_apparent_temp_c(25.0, 60.0, 18.0);
    assert_eq!(report.current.temp_c, Some(25.0));
    assert_eq!(report.current.wind_kmh, Some(18.0));
    assert_eq!(report.current.feels_like_c, Some(expected_at));
    assert!(report.current.feels_like_is_derived);
    assert_eq!(report.current.rain_mm, Some(0.4));
    assert_eq!(report.current.rain_prob_pct, Some(30.0));
    assert_eq!(report.current.summary.as_deref(), Some("partlycloudy_day"));

    // Step 1 falls back to next_6_hours when next_1_hours is absent.
    assert_eq!(report.hourly[1].rain_mm, Some(1.8));
    assert_eq!(report.hourly[1].rain_prob_pct, None);
    assert_eq!(report.hourly[1].summary.as_deref(), Some("lightrain"));
    assert!(report.hourly[1].feels_like_is_derived);
}

#[tokio::test]
async fn met_no_rejects_empty_user_agent_and_honors_expires_cache() {
    let server = MockServer::start().await;
    let node = sample_node(0);
    let t0 = Utc::now().to_rfc3339();
    let future_expires = (Utc::now() + ChronoDuration::minutes(30))
        .format("%a, %d %b %Y %H:%M:%S GMT")
        .to_string();

    Mock::given(method("GET"))
        .and(path("/weatherapi/locationforecast/2.0/compact"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Expires", future_expires.as_str())
                .set_body_json(json!({
                    "properties": {
                        "timeseries": [{
                            "time": t0,
                            "data": {
                                "instant": {
                                    "details": {
                                        "air_temperature": 18.5,
                                        "relative_humidity": 50.0,
                                        "wind_speed": 3.0
                                    }
                                }
                            }
                        }]
                    }
                })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let source = MetNorwaySource::new()
        .with_base_url(server.uri())
        .with_request_delay(Duration::ZERO);

    let empty_ua_ctx = FetchContext {
        user_agent: "   ".to_string(),
        timeout_secs: 5,
    };
    let err = source
        .fetch_mesh(std::slice::from_ref(&node), &empty_ua_ctx)
        .await
        .expect_err("empty user agent must be rejected");
    assert!(matches!(err, SourceError::Parse(_)));

    let valid_ctx = FetchContext::default();
    let first = source
        .fetch_mesh(std::slice::from_ref(&node), &valid_ctx)
        .await
        .expect("first fetch should hit server");
    let second = source
        .fetch_mesh(&[node], &valid_ctx)
        .await
        .expect("second fetch within Expires should use cache");

    assert_eq!(first.reports.len(), 1);
    assert_eq!(second.reports.len(), 1);
    assert_eq!(second.reports[0].1.current.temp_c, Some(18.5));
}
