use std::sync::Mutex;

use chrono::{TimeZone, Utc};
use serde_json::json;
use thw_core::{MeshNode, SourceKind, StateCode};
use thw_sources::keyed::{
    OpenWeatherMapSource, PirateWeatherSource, TomorrowIoSource, WeatherApiSource, ENV_OWM_KEY,
    ENV_PIRATEWEATHER_KEY, ENV_TOMORROW_KEY, ENV_WEATHERAPI_KEY,
};
use thw_sources::scraper::{parse_bom_obs_html, parse_robots_allows, BomHtmlScraperSource};
use thw_sources::{FetchContext, SourceError, WeatherSource};
use wiremock::matchers::{method, path, path_regex, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

static ENV_MUTEX: Mutex<()> = Mutex::new(());

fn make_node(id: &str, name: &str, state: StateCode, lat: f64, lon: f64, hub: bool) -> MeshNode {
    MeshNode {
        node_id: id.to_string(),
        name: name.to_string(),
        state,
        lat,
        lon,
        bom_station_id: None,
        bom_product_id: None,
        bom_forecast_product: None,
        is_major_hub: hub,
    }
}

#[tokio::test]
async fn disabled_keyed_sources_return_empty_without_failing() {
    let nodes = vec![make_node(
        "syd",
        "Sydney",
        StateCode::NSW,
        -33.8688,
        151.2093,
        true,
    )];
    let ctx = FetchContext::default();

    let weatherapi = WeatherApiSource::with_key_and_base_url("", "http://127.0.0.1:1");
    let pirate = PirateWeatherSource::with_key_and_base_url("   ", "http://127.0.0.1:1");
    let owm = OpenWeatherMapSource::with_key_and_base_url("", "http://127.0.0.1:1");
    let tomorrow = TomorrowIoSource::with_key_and_base_url("", "http://127.0.0.1:1");

    assert!(!weatherapi.is_enabled());
    assert!(!pirate.is_enabled());
    assert!(!owm.is_enabled());
    assert!(!tomorrow.is_enabled());

    assert!(weatherapi
        .fetch_mesh(&nodes, &ctx)
        .await
        .unwrap()
        .reports
        .is_empty());
    assert!(pirate
        .fetch_mesh(&nodes, &ctx)
        .await
        .unwrap()
        .reports
        .is_empty());
    assert!(owm
        .fetch_mesh(&nodes, &ctx)
        .await
        .unwrap()
        .reports
        .is_empty());
    assert!(tomorrow
        .fetch_mesh(&nodes, &ctx)
        .await
        .unwrap()
        .reports
        .is_empty());
}

#[test]
fn from_env_respects_missing_and_present_keys() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let vars = [
        ENV_WEATHERAPI_KEY,
        ENV_PIRATEWEATHER_KEY,
        ENV_OWM_KEY,
        ENV_TOMORROW_KEY,
    ];
    let saved: Vec<(&str, Option<String>)> =
        vars.iter().map(|&k| (k, std::env::var(k).ok())).collect();

    for &k in &vars {
        std::env::remove_var(k);
    }

    assert!(!WeatherApiSource::from_env().is_enabled());
    assert!(!PirateWeatherSource::from_env().is_enabled());
    assert!(!OpenWeatherMapSource::from_env().is_enabled());
    assert!(!TomorrowIoSource::from_env().is_enabled());

    std::env::set_var(ENV_WEATHERAPI_KEY, "wa-secret");
    std::env::set_var(ENV_PIRATEWEATHER_KEY, "pw-secret");
    std::env::set_var(ENV_OWM_KEY, "owm-secret");
    std::env::set_var(ENV_TOMORROW_KEY, "tm-secret");

    assert!(WeatherApiSource::from_env().is_enabled());
    assert!(PirateWeatherSource::from_env().is_enabled());
    assert!(OpenWeatherMapSource::from_env().is_enabled());
    assert!(TomorrowIoSource::from_env().is_enabled());

    for (k, v) in saved {
        match v {
            Some(val) => std::env::set_var(k, val),
            None => std::env::remove_var(k),
        }
    }
}

#[tokio::test]
async fn weatherapi_parses_current_and_hourly_forecast() {
    let server = MockServer::start().await;
    let payload = json!({
        "current": {
            "last_updated_epoch": 1764032400,
            "temp_c": 24.5,
            "feelslike_c": 23.8,
            "precip_mm": 0.2,
            "wind_kph": 18.4,
            "humidity": 62,
            "condition": { "text": "Partly cloudy" }
        },
        "forecast": {
            "forecastday": [{
                "hour": [
                    {
                        "time_epoch": 1764036000,
                        "temp_c": 25.0,
                        "feelslike_c": 24.1,
                        "chance_of_rain": 35,
                        "precip_mm": 0.4,
                        "wind_kph": 20.0,
                        "humidity": 60,
                        "condition": { "text": "Patchy rain nearby" }
                    },
                    {
                        "time_epoch": 1764039600,
                        "temp_c": 23.0,
                        "feelslike_c": null,
                        "chance_of_rain": "70",
                        "precip_mm": 1.8,
                        "wind_kph": 14.4,
                        "humidity": 78,
                        "condition": { "text": "Light rain" }
                    }
                ]
            }]
        }
    });

    Mock::given(method("GET"))
        .and(path("/v1/forecast.json"))
        .and(query_param("key", "wa-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(payload))
        .expect(1)
        .mount(&server)
        .await;

    let src = WeatherApiSource::with_key_and_base_url("wa-key", server.uri());
    let nodes = vec![make_node(
        "syd-cbd",
        "Sydney",
        StateCode::NSW,
        -33.8688,
        151.2093,
        true,
    )];

    let out = src
        .fetch_mesh(&nodes, &FetchContext::default())
        .await
        .unwrap();
    assert_eq!(out.reports.len(), 1);
    let (node_id, report) = &out.reports[0];
    assert_eq!(node_id, "syd-cbd");
    assert_eq!(report.source_id, "weatherapi");
    assert_eq!(report.current.temp_c, Some(24.5));
    assert_eq!(report.current.feels_like_c, Some(23.8));
    assert_eq!(report.current.wind_kmh, Some(18.4));
    assert_eq!(report.current.humidity_pct, Some(62.0));
    assert_eq!(report.current.summary.as_deref(), Some("Partly cloudy"));
    assert_eq!(report.hourly.len(), 2);
    assert_eq!(report.hourly[0].rain_prob_pct, Some(35.0));
    assert_eq!(report.hourly[1].rain_prob_pct, Some(70.0));
    assert!(report.hourly[1].feels_like_c.is_some());
    assert!(report.hourly[1].feels_like_is_derived);
    assert!(!report
        .source_url
        .as_deref()
        .unwrap_or("")
        .contains("wa-key"));
}

#[tokio::test]
async fn pirate_weather_parses_si_and_caps_to_major_hubs_over_15_nodes() {
    let server = MockServer::start().await;
    let payload = json!({
        "currently": {
            "time": 1764032400,
            "temperature": 21.0,
            "apparentTemperature": 20.5,
            "precipProbability": 0.25,
            "precipIntensity": 0.3,
            "windSpeed": 5.0,
            "humidity": 0.68,
            "summary": "Mostly Cloudy"
        },
        "hourly": {
            "data": [{
                "time": 1764036000,
                "temperature": 22.0,
                "apparentTemperature": 21.8,
                "precipProbability": 0.60,
                "precipIntensity": 1.2,
                "windSpeed": 4.0,
                "humidity": 0.72,
                "summary": "Light Rain"
            }]
        }
    });

    Mock::given(method("GET"))
        .and(path_regex(r"^/forecast/pw-key/-?\d+\.\d+,-?\d+\.\d+$"))
        .and(query_param("units", "si"))
        .respond_with(ResponseTemplate::new(200).set_body_json(payload))
        .expect(2)
        .mount(&server)
        .await;

    let src = PirateWeatherSource::with_key_and_base_url("pw-key", server.uri());
    let mut nodes = Vec::new();
    for i in 0..18 {
        nodes.push(make_node(
            &format!("n{i}"),
            &format!("Node {i}"),
            StateCode::VIC,
            -37.8 - (i as f64 * 0.01),
            144.9,
            i < 2,
        ));
    }

    let out = src
        .fetch_mesh(&nodes, &FetchContext::default())
        .await
        .unwrap();
    assert_eq!(out.reports.len(), 2);
    let (_, report) = &out.reports[0];
    assert_eq!(report.source_id, "pirate_weather");
    assert_eq!(report.current.temp_c, Some(21.0));
    assert_eq!(report.current.wind_kmh, Some(18.0)); // 5.0 m/s * 3.6
    assert_eq!(report.current.humidity_pct, Some(68.0));
    assert_eq!(report.current.rain_prob_pct, Some(25.0));
    assert_eq!(report.hourly.len(), 1);
    assert_eq!(report.hourly[0].wind_kmh, Some(14.4)); // 4.0 m/s * 3.6
    assert!(!report
        .source_url
        .as_deref()
        .unwrap_or("")
        .contains("pw-key"));
}

#[tokio::test]
async fn openweathermap_parses_3h_forecast_list() {
    let server = MockServer::start().await;
    let payload = json!({
        "list": [
            {
                "dt": 1764032400,
                "main": { "temp": 26.2, "feels_like": 26.8, "humidity": 58 },
                "wind": { "speed": 6.0 },
                "pop": 0.45,
                "rain": { "3h": 2.4 },
                "weather": [{ "main": "Rain", "description": "light rain" }]
            },
            {
                "dt": 1764043200,
                "main": { "temp": 24.0, "feels_like": 24.0, "humidity": 65 },
                "wind": { "speed": 3.5 },
                "pop": 0.10,
                "weather": [{ "main": "Clouds", "description": "scattered clouds" }]
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/data/2.5/forecast"))
        .and(query_param("appid", "owm-key"))
        .and(query_param("units", "metric"))
        .respond_with(ResponseTemplate::new(200).set_body_json(payload))
        .expect(1)
        .mount(&server)
        .await;

    let src = OpenWeatherMapSource::with_key_and_base_url("owm-key", server.uri());
    let nodes = vec![make_node(
        "bne",
        "Brisbane",
        StateCode::QLD,
        -27.4698,
        153.0251,
        true,
    )];

    let out = src
        .fetch_mesh(&nodes, &FetchContext::default())
        .await
        .unwrap();
    assert_eq!(out.reports.len(), 1);
    let (_, report) = &out.reports[0];
    assert_eq!(report.source_id, "openweathermap");
    assert_eq!(report.kind, SourceKind::Forecast);
    assert_eq!(report.current.temp_c, Some(26.2));
    assert_eq!(report.current.feels_like_c, Some(26.8));
    assert_eq!(report.current.wind_kmh, Some(21.6)); // 6.0 m/s * 3.6
    assert_eq!(report.current.rain_prob_pct, Some(45.0));
    assert_eq!(report.current.rain_mm, Some(2.4));
    assert_eq!(report.current.summary.as_deref(), Some("light rain"));
    assert_eq!(report.hourly.len(), 2);
}

#[tokio::test]
async fn tomorrow_io_strictly_caps_to_20_major_hubs_and_enforces_hourly_budget() {
    let server = MockServer::start().await;
    let payload = json!({
        "timelines": {
            "hourly": [
                {
                    "time": "2026-11-25T01:00:00Z",
                    "values": {
                        "temperature": 28.4,
                        "temperatureApparent": 29.1,
                        "precipitationProbability": 55,
                        "rainIntensity": 0.9,
                        "windSpeed": 5.5,
                        "humidity": 64,
                        "weatherCode": 4200
                    }
                }
            ]
        }
    });

    // Run 1: 25 major hubs + 5 non-hubs -> capped at 20 requests.
    // Run 2: 10 major hubs -> only 5 remaining in 25/hr budget -> 5 requests.
    // Run 3: 1 major hub -> RateLimited error!
    Mock::given(method("GET"))
        .and(path("/v4/weather/forecast"))
        .and(query_param("apikey", "tm-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(payload))
        .expect(25)
        .mount(&server)
        .await;

    let src = TomorrowIoSource::with_key_and_base_url("tm-key", server.uri());
    let mut nodes = Vec::new();
    for i in 0..25 {
        nodes.push(make_node(
            &format!("hub-{i}"),
            &format!("Hub {i}"),
            StateCode::NSW,
            -33.0 - (i as f64 * 0.05),
            151.0,
            true,
        ));
    }
    for i in 0..5 {
        nodes.push(make_node(
            &format!("minor-{i}"),
            &format!("Minor {i}"),
            StateCode::NSW,
            -34.0 - (i as f64 * 0.05),
            150.0,
            false,
        ));
    }

    let ctx = FetchContext::default();
    let out1 = src.fetch_mesh(&nodes, &ctx).await.unwrap();
    assert_eq!(out1.reports.len(), 20);
    assert!(out1.reports.iter().all(|(id, _)| id.starts_with("hub-")));
    let (_, sample) = &out1.reports[0];
    assert_eq!(sample.source_id, "tomorrow_io");
    assert_eq!(sample.current.temp_c, Some(28.4));
    assert_eq!(sample.current.feels_like_c, Some(29.1));
    assert_eq!(sample.current.wind_kmh, Some(19.8));
    assert_eq!(sample.current.rain_prob_pct, Some(55.0));
    assert_eq!(sample.current.rain_mm, Some(0.9));
    assert_eq!(sample.current.summary.as_deref(), Some("Light Rain"));

    // Second run consumes remaining 5 slots of the 25/hr budget.
    let out2 = src.fetch_mesh(&nodes[..10], &ctx).await.unwrap();
    assert_eq!(out2.reports.len(), 5);

    // Third run must fail fast with RateLimited without making HTTP requests.
    let err = src.fetch_mesh(&nodes[..1], &ctx).await.unwrap_err();
    assert!(matches!(err, SourceError::RateLimited(_)));
}

#[test]
fn parse_bom_obs_html_parses_multi_row_bom_table_deterministically() {
    let fixture = r#"
<!DOCTYPE html>
<html>
<head><title>Sydney Area Observations</title></head>
<body>
<table class="tabledata">
  <thead>
    <tr>
      <th rowspan="2">Station</th>
      <th rowspan="2">Date/Time</th>
      <th rowspan="2">Temp<br>&deg;C</th>
      <th rowspan="2">App Temp<br>&deg;C</th>
      <th rowspan="2">Rel Hum<br>%</th>
      <th colspan="3">Wind</th>
      <th rowspan="2">Rain since 9am<br>mm</th>
    </tr>
    <tr>
      <th>Dir</th>
      <th>Spd km/h</th>
      <th>Gust km/h</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <th class="rowleftcolumn"><a href="/products/IDN60901/IDN60901.94768.shtml">Sydney (Observatory Hill)</a></th>
      <td>25/11:30am</td>
      <td>24.6</td>
      <td>23.9</td>
      <td>58</td>
      <td>NE</td>
      <td>19</td>
      <td>26</td>
      <td>0.4</td>
    </tr>
    <tr>
      <th class="rowleftcolumn"><a href="/products/IDN60901/IDN60901.94767.shtml">Sydney Airport</a></th>
      <td>25/11:30am</td>
      <td>25.0</td>
      <td>-</td>
      <td>60</td>
      <td>CALM</td>
      <td>Calm</td>
      <td>-</td>
      <td>Trace</td>
    </tr>
    <tr>
      <th class="rowleftcolumn"><a href="/products/IDN60901/IDN60901.94765.shtml">Penrith Lakes</a></th>
      <td>25/11:30am</td>
      <td>27.2</td>
      <td>26.1</td>
      <td>45</td>
      <td>NNE</td>
      <td>11</td>
      <td>17</td>
      <td>-</td>
    </tr>
  </tbody>
</table>
</body>
</html>
"#;

    let mut syd = make_node("syd-obs", "Sydney", StateCode::NSW, -33.86, 151.20, true);
    syd.bom_station_id = Some("94768".to_string());
    let mut syd_ap = make_node(
        "syd-ap",
        "Sydney Airport",
        StateCode::NSW,
        -33.94,
        151.17,
        false,
    );
    syd_ap.bom_station_id = Some("94767".to_string());

    let fetched_at = Utc.with_ymd_and_hms(2026, 11, 25, 0, 30, 0).unwrap();
    let out = parse_bom_obs_html(fixture, &[syd, syd_ap], fetched_at);

    assert_eq!(out.reports.len(), 2);
    assert_eq!(out.observations.len(), 2);

    let (id0, rep0) = &out.reports[0];
    assert_eq!(id0, "syd-obs");
    assert_eq!(rep0.kind, SourceKind::Observation);
    assert_eq!(rep0.current.temp_c, Some(24.6));
    assert_eq!(rep0.current.feels_like_c, Some(23.9));
    assert!(!rep0.current.feels_like_is_derived);
    assert_eq!(rep0.current.humidity_pct, Some(58.0));
    assert_eq!(rep0.current.wind_kmh, Some(19.0));
    assert_eq!(rep0.current.rain_mm, Some(0.4));

    let (id1, rep1) = &out.reports[1];
    assert_eq!(id1, "syd-ap");
    assert_eq!(rep1.current.temp_c, Some(25.0));
    assert_eq!(rep1.current.wind_kmh, Some(0.0)); // Calm
    assert_eq!(rep1.current.rain_mm, Some(0.0)); // Trace
    assert!(rep1.current.feels_like_c.is_some()); // Derived via Steadman when "-"
    assert!(rep1.current.feels_like_is_derived);
}

#[tokio::test]
async fn bom_html_scraper_enforces_robots_txt_before_scraping() {
    let html_fixture = r#"
<table>
  <thead>
    <tr><th>Station</th><th>Temp °C</th><th>App Temp °C</th><th>Rel Hum %</th><th>Wind Spd km/h</th><th>Rain mm</th></tr>
  </thead>
  <tbody>
    <tr><td><a href="/products/IDN60901/IDN60901.94768.shtml">Sydney</a></td><td>22.1</td><td>21.3</td><td>64</td><td>15</td><td>0.0</td></tr>
  </tbody>
</table>
"#;

    // Case 1: robots.txt allows `/products/IDN60901/`
    let allowed_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(
                "User-agent: *\nDisallow: /cgi-bin/\nAllow: /products/IDN60901/\n",
            ),
        )
        .expect(1)
        .mount(&allowed_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/products/IDN60901/IDN60901.shtml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(html_fixture))
        .expect(1)
        .mount(&allowed_server)
        .await;

    let mut syd = make_node("syd", "Sydney", StateCode::NSW, -33.86, 151.20, true);
    syd.bom_product_id = Some("IDN60901".to_string());
    syd.bom_station_id = Some("94768".to_string());

    let scraper = BomHtmlScraperSource::with_base_url(allowed_server.uri());
    let out = scraper
        .fetch_mesh(std::slice::from_ref(&syd), &FetchContext::default())
        .await
        .unwrap();
    assert_eq!(out.reports.len(), 1);
    assert_eq!(out.observations.len(), 1);
    assert_eq!(out.reports[0].1.current.temp_c, Some(22.1));

    // Case 2: robots.txt disallows `/products/` -> product page MUST NOT be fetched
    let blocked_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("User-agent: *\nDisallow: /products/\n"),
        )
        .expect(1)
        .mount(&blocked_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/products/IDN60901/IDN60901.shtml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(html_fixture))
        .expect(0)
        .mount(&blocked_server)
        .await;

    let blocked_scraper = BomHtmlScraperSource::with_base_url(blocked_server.uri());
    let blocked_out = blocked_scraper
        .fetch_mesh(&[syd], &FetchContext::default())
        .await
        .unwrap();
    assert!(blocked_out.reports.is_empty());
    assert!(blocked_out.observations.is_empty());
}

#[test]
fn robots_policy_handles_comments_and_precedence() {
    let robots = r#"
User-agent: *
Disallow: /products/ # block everything under products
Allow: /products/IDV60901/IDV60901.shtml$
"#;

    assert!(parse_robots_allows(
        robots,
        "TurtleHeavyWeather/0.1",
        "/products/IDV60901/IDV60901.shtml"
    ));
    assert!(!parse_robots_allows(
        robots,
        "TurtleHeavyWeather/0.1",
        "/products/IDN60901/IDN60901.shtml"
    ));
}
