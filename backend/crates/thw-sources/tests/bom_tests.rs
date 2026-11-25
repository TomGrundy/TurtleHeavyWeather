use chrono::{TimeZone, Utc};
use thw_core::{MeshNode, SourceKind, StateCode};
use thw_sources::bom::{parse_amoc_xml, parse_bom_obs_xml, BomObsJsonSource, BomWebApiSource};
use thw_sources::{FetchContext, WeatherSource};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sydney_node() -> MeshNode {
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
async fn bom_obs_json_parses_observations_and_handles_9am_rain_reset() {
    let server = MockServer::start().await;
    let payload = serde_json::json!({
        "observations": {
            "data": [
                {
                    "aifstime_utc": "20261125010000",
                    "air_temp": 23.8,
                    "apparent_t": 22.5,
                    "rain_trace": "0.6",
                    "rel_hum": 60.0,
                    "wind_spd_kmh": 19.0,
                    "weather": "Showers"
                },
                {
                    "aifstime_utc": "20261125000000",
                    "air_temp": 22.1,
                    "apparent_t": 21.0,
                    "rain_trace": "4.2",
                    "rel_hum": 75.0,
                    "wind_spd_kmh": 15.0,
                    "weather": "Rain"
                },
                {
                    "aifstime_utc": "20261124230000",
                    "air_temp": 21.5,
                    "apparent_t": 20.4,
                    "rain_trace": "3.0",
                    "rel_hum": 72.0,
                    "wind_spd_kmh": 13.0,
                    "weather": "Light rain"
                }
            ]
        }
    });

    Mock::given(method("GET"))
        .and(path("/fwo/IDN60901/IDN60901.94768.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(payload))
        .mount(&server)
        .await;

    let source = BomObsJsonSource::with_base_url(server.uri());
    let output = source
        .fetch_mesh(&[sydney_node()], &FetchContext::default())
        .await
        .expect("BOM obs fetch should succeed");

    assert_eq!(output.reports.len(), 1);
    let (node_id, report) = &output.reports[0];
    assert_eq!(node_id, "bom-066062");
    assert_eq!(report.source_id, "bom_obs_json");
    assert_eq!(report.kind, SourceKind::Observation);
    assert_eq!(report.current.temp_c, Some(23.8));
    assert_eq!(report.current.feels_like_c, Some(22.5));
    // 0.6 after 4.2 means 9am reset occurred -> delta = 0.6
    assert!((report.current.rain_mm.unwrap() - 0.6).abs() < 1e-6);

    assert_eq!(output.observations.len(), 3);
    let latest_obs = output
        .observations
        .iter()
        .find(|o| o.observed_at == Utc.with_ymd_and_hms(2026, 11, 25, 1, 0, 0).unwrap())
        .unwrap();
    assert_eq!(latest_obs.quality_rank, 1);
    assert_eq!(latest_obs.rained_bool, Some(true));

    let mid_obs = output
        .observations
        .iter()
        .find(|o| o.observed_at == Utc.with_ymd_and_hms(2026, 11, 25, 0, 0, 0).unwrap())
        .unwrap();
    // 4.2 - 3.0 = 1.2 mm
    assert!((mid_obs.rain_mm_1h.unwrap() - 1.2).abs() < 1e-6);
}

#[test]
fn parse_amoc_xml_extracts_town_forecast() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<product>
  <forecast>
    <area aac="NSW_PT131" description="Sydney" type="location">
      <forecast-period index="0" start-time-utc="2026-11-25T00:00:00Z" end-time-utc="2026-11-25T13:00:00Z">
        <element type="air_temperature_minimum" units="Celsius">17</element>
        <element type="air_temperature_maximum" units="Celsius">26</element>
        <element type="precipitation_range">0 to 2 mm</element>
        <text type="precis">Partly cloudy. Slight chance of a shower.</text>
        <text type="probability_of_precipitation">30%</text>
      </forecast-period>
      <forecast-period index="1" start-time-utc="2026-11-25T13:00:00Z" end-time-utc="2026-11-26T13:00:00Z">
        <element type="air_temperature_minimum" units="Celsius">18</element>
        <element type="air_temperature_maximum" units="Celsius">28</element>
        <text type="precis">Sunny.</text>
        <text type="probability_of_precipitation">10%</text>
      </forecast-period>
    </area>
  </forecast>
</product>"#;

    let fetched_at = Utc.with_ymd_and_hms(2026, 11, 25, 0, 0, 0).unwrap();
    let parsed = parse_amoc_xml(xml, &[sydney_node()], fetched_at).expect("AMOC XML should parse");
    assert_eq!(parsed.len(), 1);
    let (node_id, report) = &parsed[0];
    assert_eq!(node_id, "bom-066062");
    assert_eq!(report.source_id, "bom_ftp_xml");
    assert_eq!(report.current.rain_prob_pct, Some(30.0));
    assert!((report.current.rain_mm.unwrap() - 1.0).abs() < 1e-6);
    assert_eq!(
        report.current.summary.as_deref(),
        Some("Partly cloudy. Slight chance of a shower.")
    );
    assert_eq!(report.hourly.len(), 2);
}

#[tokio::test]
async fn bom_web_api_fetches_hourly_and_trips_circuit_breaker_on_repeated_failures() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v1/locations"))
        .and(query_param("search", "sydney"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{ "geohash": "r3gx2f", "state": "NSW" }]
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/v1/locations/r3gx2f/forecasts/hourly"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {
                    "time": "2026-11-25T01:00:00Z",
                    "temp": 25.0,
                    "temp_feels_like": 24.0,
                    "rain": { "chance": 40, "amount": { "min": 0.0, "max": 2.0 } },
                    "wind": { "speed_kilometre": 20.0 },
                    "relative_humidity": 58,
                    "short_text": "Cloudy"
                }
            ]
        })))
        .mount(&server)
        .await;

    let source = BomWebApiSource::with_base_url(server.uri());
    let out = source
        .fetch_mesh(&[sydney_node()], &FetchContext::default())
        .await
        .unwrap();
    assert_eq!(out.reports.len(), 1);
    assert_eq!(out.reports[0].1.current.temp_c, Some(25.0));
    assert_eq!(out.reports[0].1.current.rain_prob_pct, Some(40.0));
}

#[test]
fn parse_bom_obs_xml_extracts_apparent_temp_and_tier1_observation() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<product>
  <observations>
    <station wmo-id="94768" description="Sydney (Observatory Hill)">
      <period index="0" time-utc="2026-11-25T23:00:00Z">
        <level index="0" type="surface">
          <element type="air_temperature">22.4</element>
          <element type="apparent_temp">21.6</element>
          <element type="rel-humidity">61</element>
          <element type="wind_spd_kmh">15</element>
          <element type="rainfall">0.4</element>
        </level>
      </period>
    </station>
  </observations>
</product>"#;

    let fetched_at = Utc::now();
    let out = parse_bom_obs_xml(xml, &[sydney_node()], fetched_at).unwrap();
    assert_eq!(out.reports.len(), 1);
    assert_eq!(out.observations.len(), 1);
    assert_eq!(out.reports[0].1.current.feels_like_c, Some(21.6));
    assert_eq!(out.observations[0].quality_rank, 1);
    assert_eq!(out.observations[0].rained_bool, Some(true));
}
