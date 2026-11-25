use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use reqwest::StatusCode;
use serde::Deserialize;
use thw_core::{ensure_feels_like, MeshNode, SourceKind, SourceReport, WeatherPoint};
use tracing::{debug, info, warn};

use crate::{FetchContext, SourceError, SourceOutput, WeatherSource};

pub const ENV_WEATHERAPI_KEY: &str = "THW_WEATHERAPI_KEY";
pub const ENV_PIRATEWEATHER_KEY: &str = "THW_PIRATEWEATHER_KEY";
pub const ENV_OWM_KEY: &str = "THW_OWM_KEY";
pub const ENV_TOMORROW_KEY: &str = "THW_TOMORROW_KEY";

pub const DEFAULT_WEATHERAPI_BASE: &str = "https://api.weatherapi.com";
pub const DEFAULT_PIRATEWEATHER_BASE: &str = "https://api.pirateweather.net";
pub const DEFAULT_OWM_BASE: &str = "https://api.openweathermap.org";
pub const DEFAULT_TOMORROW_BASE: &str = "https://api.tomorrow.io";

const PIRATEWEATHER_HUB_THRESHOLD: usize = 15;
const TOMORROW_MAX_NODES_PER_RUN: usize = 20;
const TOMORROW_HOURLY_BUDGET: usize = 25;
const TOMORROW_DAILY_BUDGET: usize = 500;

fn read_env_key(env_var: &str, source_id: &str) -> Option<String> {
    let raw = std::env::var(env_var).ok();
    let key = normalize_key(raw.as_deref());
    if key.is_none() {
        info!(
            source = source_id,
            env_var = env_var,
            "API key missing or empty; keyed weather source disabled"
        );
    }
    key
}

fn normalize_key(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn normalize_base_url(base_url: impl Into<String>) -> String {
    base_url.into().trim_end_matches('/').to_string()
}

fn build_client(ctx: &FetchContext) -> Result<reqwest::Client, SourceError> {
    reqwest::Client::builder()
        .user_agent(&ctx.user_agent)
        .timeout(Duration::from_secs(ctx.timeout_secs.max(1)))
        .build()
        .map_err(SourceError::Http)
}

/// Selects nodes for a source, optionally restricting to `is_major_hub` nodes
/// when `nodes.len() > hub_threshold` (or always if `hub_threshold == 0`),
/// and optionally capping total count.
fn select_nodes(
    nodes: &[MeshNode],
    hub_threshold: Option<usize>,
    max_nodes: Option<usize>,
) -> Vec<&MeshNode> {
    let filter_hubs = hub_threshold.is_some_and(|threshold| nodes.len() > threshold);
    let iter = nodes
        .iter()
        .filter(|node| !filter_hubs || node.is_major_hub);

    match max_nodes {
        Some(limit) => iter.take(limit).collect(),
        None => iter.collect(),
    }
}

/// Sliding-window rate budget guard to protect free-tier hourly/daily quotas.
#[derive(Debug)]
struct RateBudget {
    hourly_limit: usize,
    daily_limit: usize,
    timestamps: Mutex<VecDeque<Instant>>,
}

impl RateBudget {
    fn new(hourly_limit: usize, daily_limit: usize) -> Self {
        Self {
            hourly_limit,
            daily_limit,
            timestamps: Mutex::new(VecDeque::new()),
        }
    }

    fn try_acquire(&self) -> bool {
        let now = Instant::now();
        let one_hour = Duration::from_secs(3600);
        let one_day = Duration::from_secs(86400);

        let mut guard = self
            .timestamps
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        while let Some(&oldest) = guard.front() {
            if now.duration_since(oldest) >= one_day {
                guard.pop_front();
            } else {
                break;
            }
        }

        let daily_used = guard.len();
        let hourly_used = guard
            .iter()
            .filter(|&&ts| now.duration_since(ts) < one_hour)
            .count();

        if hourly_used >= self.hourly_limit || daily_used >= self.daily_limit {
            return false;
        }

        guard.push_back(now);
        true
    }
}

/// Shared per-node HTTP JSON fetch helper that handles rate-limit / error status codes
/// gracefully so one failing node does not discard healthy nodes.
async fn fetch_nodes_with_parser<F>(
    source_id: &'static str,
    nodes: &[&MeshNode],
    ctx: &FetchContext,
    mut build_request: impl FnMut(&MeshNode) -> (String, Option<String>),
    parse_body: F,
) -> Result<SourceOutput, SourceError>
where
    F: Fn(&MeshNode, &str, DateTime<Utc>) -> Result<SourceReport, SourceError>,
{
    if nodes.is_empty() {
        return Ok(SourceOutput::empty());
    }

    let client = build_client(ctx)?;
    let mut output = SourceOutput::empty();

    for (idx, node) in nodes.iter().enumerate() {
        if idx > 0 && source_id == "tomorrow_io" {
            tokio::time::sleep(Duration::from_millis(450)).await;
        }
        let (request_url, sanitized_url) = build_request(node);
        let resp = match client.get(&request_url).send().await {
            Ok(resp) => resp,
            Err(err) => {
                warn!(
                    source = source_id,
                    node_id = %node.node_id,
                    error = %err,
                    "request failed; skipping node"
                );
                continue;
            }
        };

        let status = resp.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            warn!(
                source = source_id,
                node_id = %node.node_id,
                "upstream returned 429 Too Many Requests; stopping remaining requests for this run"
            );
            break;
        }
        if !status.is_success() {
            warn!(
                source = source_id,
                node_id = %node.node_id,
                status = %status,
                "non-success HTTP status; skipping node"
            );
            continue;
        }

        let body = match resp.text().await {
            Ok(text) => text,
            Err(err) => {
                warn!(
                    source = source_id,
                    node_id = %node.node_id,
                    error = %err,
                    "failed reading response body; skipping node"
                );
                continue;
            }
        };

        let fetched_at = Utc::now();
        match parse_body(node, &body, fetched_at) {
            Ok(mut report) => {
                if report.source_url.is_none() {
                    report.source_url = sanitized_url;
                }
                output.reports.push((node.node_id.clone(), report));
            }
            Err(err) => {
                warn!(
                    source = source_id,
                    node_id = %node.node_id,
                    error = %err,
                    "failed parsing upstream JSON; skipping node"
                );
            }
        }
    }

    Ok(output)
}

fn unix_secs_to_utc(secs: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_opt(secs, 0).single()
}

fn deserialize_opt_f64_or_string<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Float(f64),
        Int(i64),
        Str(String),
    }

    let opt: Option<NumOrStr> = Option::deserialize(deserializer)?;
    match opt {
        None => Ok(None),
        Some(NumOrStr::Float(v)) => Ok(Some(v)),
        Some(NumOrStr::Int(v)) => Ok(Some(v as f64)),
        Some(NumOrStr::Str(s)) => Ok(s.trim().parse::<f64>().ok()),
    }
}

// ---------------------------------------------------------------------------
// 1. WeatherAPI.com (`THW_WEATHERAPI_KEY`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct WeatherApiSource {
    api_key: Option<String>,
    base_url: String,
}

impl WeatherApiSource {
    pub fn from_env() -> Self {
        Self {
            api_key: read_env_key(ENV_WEATHERAPI_KEY, "weatherapi"),
            base_url: DEFAULT_WEATHERAPI_BASE.to_string(),
        }
    }

    pub fn with_key_and_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        let key = api_key.into();
        Self {
            api_key: normalize_key(Some(&key)),
            base_url: normalize_base_url(base_url),
        }
    }
}

impl Default for WeatherApiSource {
    fn default() -> Self {
        Self::from_env()
    }
}

#[derive(Debug, Deserialize)]
struct WeatherApiResponse {
    current: Option<WeatherApiCurrent>,
    forecast: Option<WeatherApiForecast>,
}

#[derive(Debug, Deserialize)]
struct WeatherApiCondition {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WeatherApiCurrent {
    last_updated_epoch: Option<i64>,
    temp_c: Option<f64>,
    feelslike_c: Option<f64>,
    precip_mm: Option<f64>,
    wind_kph: Option<f64>,
    humidity: Option<f64>,
    condition: Option<WeatherApiCondition>,
}

#[derive(Debug, Deserialize)]
struct WeatherApiForecast {
    #[serde(default)]
    forecastday: Vec<WeatherApiForecastDay>,
}

#[derive(Debug, Deserialize)]
struct WeatherApiForecastDay {
    #[serde(default)]
    hour: Vec<WeatherApiHour>,
}

#[derive(Debug, Deserialize)]
struct WeatherApiHour {
    time_epoch: Option<i64>,
    temp_c: Option<f64>,
    feelslike_c: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_opt_f64_or_string")]
    chance_of_rain: Option<f64>,
    precip_mm: Option<f64>,
    wind_kph: Option<f64>,
    humidity: Option<f64>,
    condition: Option<WeatherApiCondition>,
}

fn parse_weatherapi_report(
    body: &str,
    fetched_at: DateTime<Utc>,
) -> Result<SourceReport, SourceError> {
    let parsed: WeatherApiResponse =
        serde_json::from_str(body).map_err(|e| SourceError::Parse(format!("weatherapi: {e}")))?;

    let mut hourly = Vec::new();
    if let Some(forecast) = parsed.forecast {
        for day in forecast.forecastday {
            for h in day.hour {
                let valid_at = h
                    .time_epoch
                    .and_then(unix_secs_to_utc)
                    .unwrap_or(fetched_at);
                let mut pt = WeatherPoint {
                    valid_at,
                    temp_c: h.temp_c,
                    feels_like_c: h.feelslike_c,
                    feels_like_is_derived: false,
                    rain_prob_pct: h.chance_of_rain.map(|v| v.clamp(0.0, 100.0)),
                    rain_mm: h.precip_mm,
                    wind_kmh: h.wind_kph,
                    humidity_pct: h.humidity.map(|v| v.clamp(0.0, 100.0)),
                    solar_wm2: None,
                    summary: h.condition.and_then(|c| c.text),
                };
                ensure_feels_like(&mut pt);
                hourly.push(pt);
            }
        }
    }

    let mut current = if let Some(cur) = parsed.current {
        let valid_at = cur
            .last_updated_epoch
            .and_then(unix_secs_to_utc)
            .unwrap_or(fetched_at);
        WeatherPoint {
            valid_at,
            temp_c: cur.temp_c,
            feels_like_c: cur.feelslike_c,
            feels_like_is_derived: false,
            rain_prob_pct: hourly.first().and_then(|h| h.rain_prob_pct),
            rain_mm: cur.precip_mm,
            wind_kmh: cur.wind_kph,
            humidity_pct: cur.humidity.map(|v| v.clamp(0.0, 100.0)),
            solar_wm2: None,
            summary: cur.condition.and_then(|c| c.text),
        }
    } else if let Some(first) = hourly.first() {
        first.clone()
    } else {
        return Err(SourceError::Parse(
            "weatherapi response missing both current and hourly forecast".to_string(),
        ));
    };
    ensure_feels_like(&mut current);

    Ok(SourceReport {
        source_id: "weatherapi".to_string(),
        display_name: "WeatherAPI.com".to_string(),
        kind: SourceKind::Hybrid,
        fetched_at,
        weight: 0.85,
        current,
        hourly,
        source_url: None,
    })
}

#[async_trait]
impl WeatherSource for WeatherApiSource {
    fn source_id(&self) -> &'static str {
        "weatherapi"
    }

    fn display_name(&self) -> &'static str {
        "WeatherAPI.com"
    }

    fn default_weight(&self) -> f64 {
        0.85
    }

    fn is_enabled(&self) -> bool {
        self.api_key.is_some()
    }

    async fn fetch_mesh(
        &self,
        nodes: &[MeshNode],
        ctx: &FetchContext,
    ) -> Result<SourceOutput, SourceError> {
        let Some(key) = &self.api_key else {
            info!(source = self.source_id(), "source disabled; skipping fetch");
            return Ok(SourceOutput::empty());
        };

        let selected = select_nodes(nodes, None, None);
        fetch_nodes_with_parser(
            self.source_id(),
            &selected,
            ctx,
            |node| {
                let req_url = format!(
                    "{}/v1/forecast.json?key={}&q={:.4},{:.4}&days=3&aqi=no&alerts=no",
                    self.base_url, key, node.lat, node.lon
                );
                let safe_url = format!(
                    "{}/v1/forecast.json?q={:.4},{:.4}",
                    self.base_url, node.lat, node.lon
                );
                (req_url, Some(safe_url))
            },
            |_node, body, fetched_at| parse_weatherapi_report(body, fetched_at),
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// 2. Pirate Weather (`THW_PIRATEWEATHER_KEY`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PirateWeatherSource {
    api_key: Option<String>,
    base_url: String,
}

impl PirateWeatherSource {
    pub fn from_env() -> Self {
        Self {
            api_key: read_env_key(ENV_PIRATEWEATHER_KEY, "pirate_weather"),
            base_url: DEFAULT_PIRATEWEATHER_BASE.to_string(),
        }
    }

    pub fn with_key_and_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        let key = api_key.into();
        Self {
            api_key: normalize_key(Some(&key)),
            base_url: normalize_base_url(base_url),
        }
    }
}

impl Default for PirateWeatherSource {
    fn default() -> Self {
        Self::from_env()
    }
}

#[derive(Debug, Deserialize)]
struct PirateWeatherResponse {
    currently: Option<DarkSkyPoint>,
    hourly: Option<DarkSkyHourly>,
}

#[derive(Debug, Deserialize)]
struct DarkSkyHourly {
    #[serde(default)]
    data: Vec<DarkSkyPoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DarkSkyPoint {
    time: Option<i64>,
    temperature: Option<f64>,
    apparent_temperature: Option<f64>,
    precip_probability: Option<f64>,
    precip_intensity: Option<f64>,
    wind_speed: Option<f64>,
    humidity: Option<f64>,
    summary: Option<String>,
}

fn darksky_to_weather_point(pt: DarkSkyPoint, fallback_time: DateTime<Utc>) -> WeatherPoint {
    let valid_at = pt.time.and_then(unix_secs_to_utc).unwrap_or(fallback_time);
    let mut wp = WeatherPoint {
        valid_at,
        temp_c: pt.temperature,
        feels_like_c: pt.apparent_temperature,
        feels_like_is_derived: false,
        rain_prob_pct: pt.precip_probability.map(|p| (p * 100.0).clamp(0.0, 100.0)),
        rain_mm: pt.precip_intensity,
        // `units=si` returns windSpeed in m/s -> convert to km/h
        wind_kmh: pt.wind_speed.map(|ms| (ms * 3.6 * 10.0).round() / 10.0),
        humidity_pct: pt.humidity.map(|h| (h * 100.0).clamp(0.0, 100.0)),
        solar_wm2: None,
        summary: pt.summary,
    };
    ensure_feels_like(&mut wp);
    wp
}

fn parse_pirateweather_report(
    body: &str,
    fetched_at: DateTime<Utc>,
) -> Result<SourceReport, SourceError> {
    let parsed: PirateWeatherResponse = serde_json::from_str(body)
        .map_err(|e| SourceError::Parse(format!("pirate_weather: {e}")))?;

    let hourly: Vec<WeatherPoint> = parsed
        .hourly
        .map(|h| {
            h.data
                .into_iter()
                .map(|pt| darksky_to_weather_point(pt, fetched_at))
                .collect()
        })
        .unwrap_or_default();

    let current = if let Some(cur) = parsed.currently {
        darksky_to_weather_point(cur, fetched_at)
    } else if let Some(first) = hourly.first() {
        first.clone()
    } else {
        return Err(SourceError::Parse(
            "pirate_weather response missing both currently and hourly.data".to_string(),
        ));
    };

    Ok(SourceReport {
        source_id: "pirate_weather".to_string(),
        display_name: "Pirate Weather".to_string(),
        kind: SourceKind::Hybrid,
        fetched_at,
        weight: 0.85,
        current,
        hourly,
        source_url: None,
    })
}

#[async_trait]
impl WeatherSource for PirateWeatherSource {
    fn source_id(&self) -> &'static str {
        "pirate_weather"
    }

    fn display_name(&self) -> &'static str {
        "Pirate Weather"
    }

    fn default_weight(&self) -> f64 {
        0.85
    }

    fn is_enabled(&self) -> bool {
        self.api_key.is_some()
    }

    async fn fetch_mesh(
        &self,
        nodes: &[MeshNode],
        ctx: &FetchContext,
    ) -> Result<SourceOutput, SourceError> {
        let Some(key) = &self.api_key else {
            info!(source = self.source_id(), "source disabled; skipping fetch");
            return Ok(SourceOutput::empty());
        };

        let selected = select_nodes(nodes, Some(PIRATEWEATHER_HUB_THRESHOLD), None);
        if nodes.len() > PIRATEWEATHER_HUB_THRESHOLD {
            debug!(
                source = self.source_id(),
                total_nodes = nodes.len(),
                selected_hubs = selected.len(),
                "mesh exceeds 15 nodes; capping Pirate Weather to major hubs"
            );
        }

        fetch_nodes_with_parser(
            self.source_id(),
            &selected,
            ctx,
            |node| {
                let req_url = format!(
                    "{}/forecast/{}/{:.4},{:.4}?units=si",
                    self.base_url, key, node.lat, node.lon
                );
                let safe_url = format!(
                    "{}/forecast/REDACTED/{:.4},{:.4}?units=si",
                    self.base_url, node.lat, node.lon
                );
                (req_url, Some(safe_url))
            },
            |_node, body, fetched_at| parse_pirateweather_report(body, fetched_at),
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// 3. OpenWeatherMap (`THW_OWM_KEY`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct OpenWeatherMapSource {
    api_key: Option<String>,
    base_url: String,
}

impl OpenWeatherMapSource {
    pub fn from_env() -> Self {
        Self {
            api_key: read_env_key(ENV_OWM_KEY, "openweathermap"),
            base_url: DEFAULT_OWM_BASE.to_string(),
        }
    }

    pub fn with_key_and_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        let key = api_key.into();
        Self {
            api_key: normalize_key(Some(&key)),
            base_url: normalize_base_url(base_url),
        }
    }
}

impl Default for OpenWeatherMapSource {
    fn default() -> Self {
        Self::from_env()
    }
}

#[derive(Debug, Deserialize)]
struct OwmForecastResponse {
    #[serde(default)]
    list: Vec<OwmForecastItem>,
}

#[derive(Debug, Deserialize)]
struct OwmForecastItem {
    dt: i64,
    main: OwmMain,
    wind: Option<OwmWind>,
    pop: Option<f64>,
    rain: Option<OwmRain>,
    #[serde(default)]
    weather: Vec<OwmWeather>,
}

#[derive(Debug, Deserialize)]
struct OwmMain {
    temp: Option<f64>,
    feels_like: Option<f64>,
    humidity: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct OwmWind {
    speed: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct OwmRain {
    #[serde(rename = "1h")]
    one_h: Option<f64>,
    #[serde(rename = "3h")]
    three_h: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct OwmWeather {
    main: Option<String>,
    description: Option<String>,
}

fn parse_owm_report(body: &str, fetched_at: DateTime<Utc>) -> Result<SourceReport, SourceError> {
    let parsed: OwmForecastResponse = serde_json::from_str(body)
        .map_err(|e| SourceError::Parse(format!("openweathermap: {e}")))?;

    if parsed.list.is_empty() {
        return Err(SourceError::Parse(
            "openweathermap forecast list is empty".to_string(),
        ));
    }

    let hourly: Vec<WeatherPoint> = parsed
        .list
        .into_iter()
        .map(|item| {
            let valid_at = unix_secs_to_utc(item.dt).unwrap_or(fetched_at);
            let summary = item
                .weather
                .into_iter()
                .next()
                .and_then(|w| w.description.or(w.main));
            let rain_mm = item.rain.and_then(|r| r.one_h.or(r.three_h));
            let wind_kmh = item
                .wind
                .and_then(|w| w.speed)
                .map(|ms| (ms * 3.6 * 10.0).round() / 10.0);

            let mut wp = WeatherPoint {
                valid_at,
                temp_c: item.main.temp,
                feels_like_c: item.main.feels_like,
                feels_like_is_derived: false,
                rain_prob_pct: item.pop.map(|p| (p * 100.0).clamp(0.0, 100.0)),
                rain_mm,
                wind_kmh,
                humidity_pct: item.main.humidity.map(|h| h.clamp(0.0, 100.0)),
                solar_wm2: None,
                summary,
            };
            ensure_feels_like(&mut wp);
            wp
        })
        .collect();

    let current = hourly[0].clone();

    Ok(SourceReport {
        source_id: "openweathermap".to_string(),
        display_name: "OpenWeatherMap".to_string(),
        kind: SourceKind::Forecast,
        fetched_at,
        weight: 0.80,
        current,
        hourly,
        source_url: None,
    })
}

#[async_trait]
impl WeatherSource for OpenWeatherMapSource {
    fn source_id(&self) -> &'static str {
        "openweathermap"
    }

    fn display_name(&self) -> &'static str {
        "OpenWeatherMap"
    }

    fn default_weight(&self) -> f64 {
        0.80
    }

    fn is_enabled(&self) -> bool {
        self.api_key.is_some()
    }

    async fn fetch_mesh(
        &self,
        nodes: &[MeshNode],
        ctx: &FetchContext,
    ) -> Result<SourceOutput, SourceError> {
        let Some(key) = &self.api_key else {
            info!(source = self.source_id(), "source disabled; skipping fetch");
            return Ok(SourceOutput::empty());
        };

        let selected = select_nodes(nodes, None, None);
        fetch_nodes_with_parser(
            self.source_id(),
            &selected,
            ctx,
            |node| {
                let req_url = format!(
                    "{}/data/2.5/forecast?lat={:.4}&lon={:.4}&appid={}&units=metric",
                    self.base_url, node.lat, node.lon, key
                );
                let safe_url = format!(
                    "{}/data/2.5/forecast?lat={:.4}&lon={:.4}&units=metric",
                    self.base_url, node.lat, node.lon
                );
                (req_url, Some(safe_url))
            },
            |_node, body, fetched_at| parse_owm_report(body, fetched_at),
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// 4. Tomorrow.io (`THW_TOMORROW_KEY`)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct TomorrowIoSource {
    api_key: Option<String>,
    base_url: String,
    budget: RateBudget,
}

impl TomorrowIoSource {
    pub fn from_env() -> Self {
        Self {
            api_key: read_env_key(ENV_TOMORROW_KEY, "tomorrow_io"),
            base_url: DEFAULT_TOMORROW_BASE.to_string(),
            budget: RateBudget::new(TOMORROW_HOURLY_BUDGET, TOMORROW_DAILY_BUDGET),
        }
    }

    pub fn with_key_and_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        let key = api_key.into();
        Self {
            api_key: normalize_key(Some(&key)),
            base_url: normalize_base_url(base_url),
            budget: RateBudget::new(TOMORROW_HOURLY_BUDGET, TOMORROW_DAILY_BUDGET),
        }
    }
}

impl Default for TomorrowIoSource {
    fn default() -> Self {
        Self::from_env()
    }
}

#[derive(Debug, Deserialize)]
struct TomorrowResponse {
    timelines: Option<TomorrowTimelines>,
}

#[derive(Debug, Deserialize)]
struct TomorrowTimelines {
    #[serde(default)]
    minutely: Vec<TomorrowInterval>,
    #[serde(default)]
    hourly: Vec<TomorrowInterval>,
}

#[derive(Debug, Deserialize)]
struct TomorrowInterval {
    time: String,
    values: TomorrowValues,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TomorrowValues {
    temperature: Option<f64>,
    temperature_apparent: Option<f64>,
    precipitation_probability: Option<f64>,
    rain_intensity: Option<f64>,
    precipitation_intensity: Option<f64>,
    wind_speed: Option<f64>,
    humidity: Option<f64>,
    weather_code: Option<i32>,
}

fn tomorrow_weather_code_summary(code: i32) -> Option<&'static str> {
    match code {
        1000 => Some("Clear"),
        1100 => Some("Mostly Clear"),
        1101 => Some("Partly Cloudy"),
        1102 => Some("Mostly Cloudy"),
        1001 => Some("Cloudy"),
        2000 | 2100 => Some("Fog"),
        4000 => Some("Drizzle"),
        4001 => Some("Rain"),
        4200 => Some("Light Rain"),
        4201 => Some("Heavy Rain"),
        8000 => Some("Thunderstorm"),
        _ => None,
    }
}

fn tomorrow_interval_to_point(
    interval: TomorrowInterval,
    fallback_time: DateTime<Utc>,
) -> WeatherPoint {
    let valid_at = DateTime::parse_from_rfc3339(&interval.time)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or(fallback_time);
    let vals = interval.values;
    let rain_mm = vals.rain_intensity.or(vals.precipitation_intensity);
    let summary = vals
        .weather_code
        .and_then(tomorrow_weather_code_summary)
        .map(str::to_string);

    let mut wp = WeatherPoint {
        valid_at,
        temp_c: vals.temperature,
        feels_like_c: vals.temperature_apparent,
        feels_like_is_derived: false,
        rain_prob_pct: vals.precipitation_probability.map(|p| p.clamp(0.0, 100.0)),
        rain_mm,
        // `units=metric` returns windSpeed in m/s -> convert to km/h
        wind_kmh: vals.wind_speed.map(|ms| (ms * 3.6 * 10.0).round() / 10.0),
        humidity_pct: vals.humidity.map(|h| h.clamp(0.0, 100.0)),
        solar_wm2: None,
        summary,
    };
    ensure_feels_like(&mut wp);
    wp
}

fn parse_tomorrow_report(
    body: &str,
    fetched_at: DateTime<Utc>,
) -> Result<SourceReport, SourceError> {
    let parsed: TomorrowResponse =
        serde_json::from_str(body).map_err(|e| SourceError::Parse(format!("tomorrow_io: {e}")))?;

    let timelines = parsed
        .timelines
        .ok_or_else(|| SourceError::Parse("tomorrow_io response missing timelines".to_string()))?;

    let minutely: Vec<WeatherPoint> = timelines
        .minutely
        .into_iter()
        .map(|i| tomorrow_interval_to_point(i, fetched_at))
        .collect();
    let hourly: Vec<WeatherPoint> = timelines
        .hourly
        .into_iter()
        .map(|i| tomorrow_interval_to_point(i, fetched_at))
        .collect();

    let current = minutely
        .first()
        .or_else(|| hourly.first())
        .cloned()
        .ok_or_else(|| {
            SourceError::Parse("tomorrow_io response has no hourly or minutely entries".to_string())
        })?;

    Ok(SourceReport {
        source_id: "tomorrow_io".to_string(),
        display_name: "Tomorrow.io".to_string(),
        kind: SourceKind::Forecast,
        fetched_at,
        weight: 0.85,
        current,
        hourly,
        source_url: None,
    })
}

#[async_trait]
impl WeatherSource for TomorrowIoSource {
    fn source_id(&self) -> &'static str {
        "tomorrow_io"
    }

    fn display_name(&self) -> &'static str {
        "Tomorrow.io"
    }

    fn default_weight(&self) -> f64 {
        0.85
    }

    fn is_enabled(&self) -> bool {
        self.api_key.is_some()
    }

    async fn fetch_mesh(
        &self,
        nodes: &[MeshNode],
        ctx: &FetchContext,
    ) -> Result<SourceOutput, SourceError> {
        let Some(key) = &self.api_key else {
            info!(source = self.source_id(), "source disabled; skipping fetch");
            return Ok(SourceOutput::empty());
        };

        // Strictly cap to `is_major_hub` nodes & max 20 nodes/run (25/hr, 500/day free cap).
        let hub_nodes = select_nodes(nodes, Some(0), Some(TOMORROW_MAX_NODES_PER_RUN));
        if hub_nodes.is_empty() {
            return Ok(SourceOutput::empty());
        }

        let mut budgeted_nodes = Vec::with_capacity(hub_nodes.len());
        for node in hub_nodes {
            if !self.budget.try_acquire() {
                warn!(
                    source = self.source_id(),
                    "Tomorrow.io rate budget reached (25/hr or 500/day); halting remaining requests"
                );
                break;
            }
            budgeted_nodes.push(node);
        }

        if budgeted_nodes.is_empty() {
            return Err(SourceError::RateLimited(self.source_id().to_string()));
        }

        fetch_nodes_with_parser(
            self.source_id(),
            &budgeted_nodes,
            ctx,
            |node| {
                let req_url = format!(
                    "{}/v4/weather/forecast?location={:.4},{:.4}&timesteps=1h&units=metric&apikey={}",
                    self.base_url, node.lat, node.lon, key
                );
                let safe_url = format!(
                    "{}/v4/weather/forecast?location={:.4},{:.4}&timesteps=1h&units=metric",
                    self.base_url, node.lat, node.lon
                );
                (req_url, Some(safe_url))
            },
            |_node, body, fetched_at| parse_tomorrow_report(body, fetched_at),
        )
        .await
    }
}
