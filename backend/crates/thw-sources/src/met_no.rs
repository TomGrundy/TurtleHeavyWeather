use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::header::{EXPIRES, IF_MODIFIED_SINCE, LAST_MODIFIED, USER_AGENT};
use serde::Deserialize;
use thw_core::{ensure_feels_like, MeshNode, SourceKind, SourceReport, WeatherPoint};
use tokio::sync::Mutex;

use crate::{FetchContext, SourceError, SourceOutput, WeatherSource};

pub const DEFAULT_MET_NO_BASE_URL: &str = "https://api.met.no";
pub const DEFAULT_MET_NO_DELAY_MS: u64 = 60;

#[derive(Debug, Clone)]
struct CachedNodeForecast {
    expires_at: Option<DateTime<Utc>>,
    last_modified: Option<String>,
    report: SourceReport,
}

/// WeatherSource adapter for MET Norway Locationforecast 2.0 (`compact`).
///
/// Enforces a descriptive `User-Agent` header on every request, throttles
/// sequential requests politely, honors `Expires` / `If-Modified-Since` caching,
/// and derives `feels_like_c` via `thw_core::ensure_feels_like`.
#[derive(Debug, Clone)]
pub struct MetNorwaySource {
    base_url: String,
    request_delay: Duration,
    enabled: bool,
    cache: Arc<Mutex<HashMap<String, CachedNodeForecast>>>,
}

impl Default for MetNorwaySource {
    fn default() -> Self {
        Self::new()
    }
}

impl MetNorwaySource {
    /// Creates an enabled `MetNorwaySource` targeting `https://api.met.no`.
    pub fn new() -> Self {
        Self {
            base_url: DEFAULT_MET_NO_BASE_URL.to_string(),
            request_delay: Duration::from_millis(DEFAULT_MET_NO_DELAY_MS),
            enabled: true,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Overrides the API base URL (useful for wiremock tests).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }

    /// Sets the polite inter-request pacing delay between sequential node queries.
    pub fn with_request_delay(mut self, delay: Duration) -> Self {
        self.request_delay = delay;
        self
    }

    /// Enables or disables this source adapter.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn build_node_url(&self, node: &MeshNode) -> (String, String) {
        let lat = round_coord_4dp(node.lat);
        let lon = round_coord_4dp(node.lon);
        let key = format!("{lat},{lon}");
        let url = format!(
            "{}/weatherapi/locationforecast/2.0/compact?lat={}&lon={}",
            self.base_url, lat, lon
        );
        (key, url)
    }
}

#[async_trait]
impl WeatherSource for MetNorwaySource {
    fn source_id(&self) -> &'static str {
        "met_no"
    }

    fn display_name(&self) -> &'static str {
        "MET Norway"
    }

    fn default_weight(&self) -> f64 {
        1.05
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn fetch_mesh(
        &self,
        nodes: &[MeshNode],
        ctx: &FetchContext,
    ) -> Result<SourceOutput, SourceError> {
        if !self.enabled {
            return Err(SourceError::Disabled(self.source_id().to_string()));
        }
        if nodes.is_empty() {
            return Ok(SourceOutput::empty());
        }

        let ua = ctx.user_agent.trim();
        if ua.is_empty() {
            return Err(SourceError::Parse(
                "MET Norway requires a non-empty User-Agent header".to_string(),
            ));
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(ctx.timeout_secs))
            .build()?;

        let mut output = SourceOutput::empty();
        let mut last_error: Option<SourceError> = None;
        let mut sent_http_request = false;

        for node in nodes {
            let now = Utc::now();
            let (cache_key, url) = self.build_node_url(node);

            let cached = {
                let guard = self.cache.lock().await;
                guard.get(&cache_key).cloned()
            };

            if let Some(entry) = &cached {
                if entry.expires_at.is_some_and(|exp| now < exp) {
                    let mut report = entry.report.clone();
                    report.fetched_at = now;
                    output.reports.push((node.node_id.clone(), report));
                    continue;
                }
            }

            if sent_http_request && !self.request_delay.is_zero() {
                tokio::time::sleep(self.request_delay).await;
            }
            sent_http_request = true;

            let mut req = client.get(&url).header(USER_AGENT, ua);
            if let Some(entry) = &cached {
                if let Some(last_mod) = &entry.last_modified {
                    req = req.header(IF_MODIFIED_SINCE, last_mod);
                }
            }

            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        source_id = self.source_id(),
                        node_id = %node.node_id,
                        error = %e,
                        "MET Norway HTTP request failed"
                    );
                    last_error = Some(SourceError::Http(e));
                    continue;
                }
            };

            if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(SourceError::RateLimited(self.source_id().to_string()));
            }

            if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
                if let Some(mut entry) = cached {
                    let new_expires = parse_http_expires(resp.headers().get(EXPIRES));
                    if new_expires.is_some() {
                        entry.expires_at = new_expires;
                        let mut guard = self.cache.lock().await;
                        guard.insert(cache_key, entry.clone());
                    }
                    let mut report = entry.report;
                    report.fetched_at = now;
                    output.reports.push((node.node_id.clone(), report));
                    continue;
                }
            }

            if let Err(e) = resp.error_for_status_ref() {
                tracing::warn!(
                    source_id = self.source_id(),
                    node_id = %node.node_id,
                    status = %resp.status(),
                    "MET Norway returned error status"
                );
                last_error = Some(SourceError::Http(e));
                continue;
            }

            let expires_at = parse_http_expires(resp.headers().get(EXPIRES));
            let last_modified = resp
                .headers()
                .get(LAST_MODIFIED)
                .and_then(|v| v.to_str().ok())
                .map(ToString::to_string);

            let body = match resp.text().await {
                Ok(b) => b,
                Err(e) => {
                    last_error = Some(SourceError::Http(e));
                    continue;
                }
            };

            let payload: MetNoResponse = match serde_json::from_str(&body) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(
                        source_id = self.source_id(),
                        node_id = %node.node_id,
                        error = %e,
                        "failed to parse MET Norway JSON"
                    );
                    last_error = Some(SourceError::Parse(format!(
                        "failed to parse MET Norway response: {e}"
                    )));
                    continue;
                }
            };

            let hourly = parse_timeseries(&payload.properties.timeseries);
            let Some(current) = select_nearest_point(&hourly, now) else {
                tracing::warn!(
                    source_id = self.source_id(),
                    node_id = %node.node_id,
                    "MET Norway returned empty or unparseable timeseries"
                );
                last_error = Some(SourceError::Parse(
                    "MET Norway timeseries contained no valid entries".to_string(),
                ));
                continue;
            };

            let report = SourceReport {
                source_id: self.source_id().to_string(),
                display_name: self.display_name().to_string(),
                kind: SourceKind::Forecast,
                fetched_at: now,
                weight: self.default_weight(),
                current,
                hourly,
                source_url: Some(url),
            };

            {
                let mut guard = self.cache.lock().await;
                guard.insert(
                    cache_key,
                    CachedNodeForecast {
                        expires_at,
                        last_modified,
                        report: report.clone(),
                    },
                );
            }

            output.reports.push((node.node_id.clone(), report));
        }

        if output.reports.is_empty() {
            if let Some(err) = last_error {
                return Err(err);
            }
        }

        Ok(output)
    }
}

#[derive(Debug, Deserialize)]
struct MetNoResponse {
    properties: MetNoProperties,
}

#[derive(Debug, Deserialize)]
struct MetNoProperties {
    #[serde(default)]
    timeseries: Vec<MetNoTimeseriesEntry>,
}

#[derive(Debug, Deserialize)]
struct MetNoTimeseriesEntry {
    time: String,
    data: MetNoData,
}

#[derive(Debug, Deserialize)]
struct MetNoData {
    instant: Option<MetNoInstant>,
    next_1_hours: Option<MetNoPeriod>,
    next_6_hours: Option<MetNoPeriod>,
}

#[derive(Debug, Deserialize)]
struct MetNoInstant {
    details: Option<MetNoInstantDetails>,
}

#[derive(Debug, Deserialize)]
struct MetNoInstantDetails {
    air_temperature: Option<f64>,
    relative_humidity: Option<f64>,
    wind_speed: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct MetNoPeriod {
    summary: Option<MetNoSummary>,
    details: Option<MetNoPeriodDetails>,
}

#[derive(Debug, Deserialize)]
struct MetNoSummary {
    symbol_code: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MetNoPeriodDetails {
    precipitation_amount: Option<f64>,
    probability_of_precipitation: Option<f64>,
}

fn parse_timeseries(entries: &[MetNoTimeseriesEntry]) -> Vec<WeatherPoint> {
    let mut points = Vec::with_capacity(entries.len());

    for entry in entries {
        let Ok(dt) = DateTime::parse_from_rfc3339(&entry.time) else {
            tracing::warn!(timestamp = %entry.time, "skipping unparseable MET Norway timestamp");
            continue;
        };
        let valid_at = dt.with_timezone(&Utc);

        let instant = entry.data.instant.as_ref().and_then(|i| i.details.as_ref());
        let temp_c = instant.and_then(|d| d.air_temperature);
        let humidity_pct = instant.and_then(|d| d.relative_humidity);
        let wind_kmh = instant
            .and_then(|d| d.wind_speed)
            .map(|ms| (ms * 3.6 * 10.0).round() / 10.0);

        let period = entry
            .data
            .next_1_hours
            .as_ref()
            .or(entry.data.next_6_hours.as_ref());
        let rain_mm = period
            .and_then(|p| p.details.as_ref())
            .and_then(|d| d.precipitation_amount);
        let rain_prob_pct = period
            .and_then(|p| p.details.as_ref())
            .and_then(|d| d.probability_of_precipitation);
        let summary = period
            .and_then(|p| p.summary.as_ref())
            .and_then(|s| s.symbol_code.clone());

        let mut point = WeatherPoint {
            valid_at,
            temp_c,
            feels_like_c: None,
            feels_like_is_derived: false,
            rain_prob_pct,
            rain_mm,
            wind_kmh,
            humidity_pct,
            solar_wm2: None,
            summary,
        };
        ensure_feels_like(&mut point);
        points.push(point);
    }

    points
}

fn select_nearest_point(points: &[WeatherPoint], target: DateTime<Utc>) -> Option<WeatherPoint> {
    points
        .iter()
        .min_by_key(|p| (p.valid_at - target).num_seconds().abs())
        .cloned()
}

fn round_coord_4dp(val: f64) -> f64 {
    (val * 10_000.0).round() / 10_000.0
}

fn parse_http_expires(header: Option<&reqwest::header::HeaderValue>) -> Option<DateTime<Utc>> {
    let raw = header?.to_str().ok()?.trim();
    if let Ok(dt) = DateTime::parse_from_rfc2822(raw) {
        return Some(dt.with_timezone(&Utc));
    }
    // RFC 2822 / HTTP-date often ends with " GMT" rather than "+0000".
    if let Some(prefix) = raw.strip_suffix(" GMT") {
        let with_offset = format!("{prefix} +0000");
        if let Ok(dt) = DateTime::parse_from_rfc2822(&with_offset) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    None
}
