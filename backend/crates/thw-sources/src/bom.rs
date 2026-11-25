use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, NaiveDateTime, Utc};
use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::Client;
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use suppaftp::types::FileType;
use suppaftp::{FtpStream, Mode};
use thw_core::{
    ensure_feels_like, MeshNode, ObservationRecord, SourceKind, SourceReport, StateCode,
    WeatherPoint,
};
use tracing::{debug, warn};

use crate::{FetchContext, SourceError, SourceOutput, WeatherSource};

pub const DEFAULT_OBS_BASE_URL: &str = "http://www.bom.gov.au";
pub const DEFAULT_FTP_HOST: &str = "ftp.bom.gov.au:21";
pub const DEFAULT_FTP_DIR: &str = "/anon/gen/fwo";
pub const DEFAULT_WEB_API_BASE_URL: &str = "https://api.weather.bom.gov.au";

const SOURCE_ID_OBS_JSON: &str = "bom_obs_json";
const SOURCE_ID_FTP_XML: &str = "bom_ftp_xml";
const SOURCE_ID_WEB_API: &str = "bom_web_api";

fn build_http_client(ctx: &FetchContext) -> Result<Client, SourceError> {
    Client::builder()
        .user_agent(&ctx.user_agent)
        .timeout(Duration::from_secs(ctx.timeout_secs.max(1)))
        .build()
        .map_err(SourceError::Http)
}

fn round_3dp(val: f64) -> f64 {
    (val * 1000.0).round() / 1000.0
}

fn clean_bom_text(raw: Option<String>) -> Option<String> {
    raw.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() || trimmed == "-" || trimmed == "--" || trimmed.eq_ignore_ascii_case("n/a") {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// Custom serde deserializer for BOM fields that may arrive as a JSON number,
/// a numeric string (`"1.2"`), a dash sentinel (`"-"`), empty string, or `null`.
fn deserialize_opt_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let val = Option::<serde_json::Value>::deserialize(deserializer)?;
    match val {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(n)) => Ok(n.as_f64()),
        Some(serde_json::Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty()
                || trimmed == "-"
                || trimmed == "--"
                || trimmed.eq_ignore_ascii_case("n/a")
            {
                Ok(None)
            } else {
                Ok(trimmed.parse::<f64>().ok())
            }
        }
        Some(_) => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// 1. BomObsJsonSource — Public JSON Station Observations
// ---------------------------------------------------------------------------

/// Adapter for Bureau of Meteorology public station observation JSON feeds:
/// `{base_url}/fwo/{product_id}/{product_id}.{station_id}.json`.
#[derive(Debug, Clone)]
pub struct BomObsJsonSource {
    base_url: String,
}

impl Default for BomObsJsonSource {
    fn default() -> Self {
        Self::new()
    }
}

impl BomObsJsonSource {
    /// Creates a new `BomObsJsonSource` targeting the live BOM endpoint.
    pub fn new() -> Self {
        Self {
            base_url: DEFAULT_OBS_BASE_URL.to_string(),
        }
    }

    /// Creates a `BomObsJsonSource` with a custom base URL (e.g. for `wiremock` tests).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    fn station_url(&self, product_id: &str, station_id: &str) -> String {
        format!(
            "{}/fwo/{}/{}.{}.json",
            self.base_url, product_id, product_id, station_id
        )
    }
}

#[derive(Debug, Deserialize)]
struct BomObsEnvelope {
    observations: BomObsBody,
}

#[derive(Debug, Deserialize)]
struct BomObsBody {
    #[serde(default)]
    data: Vec<BomObsRow>,
}

#[derive(Debug, Deserialize)]
struct BomObsRow {
    #[serde(default)]
    sort_order: Option<i64>,
    aifstime_utc: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_f64")]
    air_temp: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_opt_f64")]
    apparent_t: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_opt_f64")]
    rain_trace: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_opt_f64")]
    rel_hum: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_opt_f64")]
    wind_spd_kmh: Option<f64>,
    #[serde(default)]
    weather: Option<String>,
}

struct ParsedObsRow {
    sort_order: i64,
    valid_at: DateTime<Utc>,
    temp_c: Option<f64>,
    feels_like_c: Option<f64>,
    rain_trace: Option<f64>,
    humidity_pct: Option<f64>,
    wind_kmh: Option<f64>,
    summary: Option<String>,
}

fn parse_aifstime_utc(raw: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(raw.trim(), "%Y%m%d%H%M%S")
        .ok()
        .map(|ndt| ndt.and_utc())
}

/// Parses a BOM station observations JSON payload into a `SourceReport` and
/// ground-truth `ObservationRecord` entries, computing hourly rainfall deltas
/// from cumulative `rain_trace` (handling the 9am local reset).
pub fn parse_obs_json(
    json_str: &str,
    node: &MeshNode,
    fetched_at: DateTime<Utc>,
    source_url: Option<String>,
) -> Result<(SourceReport, Vec<ObservationRecord>), SourceError> {
    let envelope: BomObsEnvelope = serde_json::from_str(json_str)
        .map_err(|e| SourceError::Parse(format!("BOM obs JSON parse error: {e}")))?;

    let mut rows: Vec<ParsedObsRow> = envelope
        .observations
        .data
        .into_iter()
        .filter_map(|row| {
            let valid_at = row.aifstime_utc.as_deref().and_then(parse_aifstime_utc)?;
            Some(ParsedObsRow {
                sort_order: row.sort_order.unwrap_or(i64::MAX),
                valid_at,
                temp_c: row.air_temp,
                feels_like_c: row.apparent_t,
                rain_trace: row.rain_trace,
                humidity_pct: row.rel_hum,
                wind_kmh: row.wind_spd_kmh,
                summary: clean_bom_text(row.weather),
            })
        })
        .collect();

    if rows.is_empty() {
        return Err(SourceError::Parse(format!(
            "BOM observations.data contained no valid timestamped rows for {}",
            node.node_id
        )));
    }

    // Sort chronologically ascending; for identical timestamps keep lowest sort_order first.
    rows.sort_by(|a, b| {
        a.valid_at
            .cmp(&b.valid_at)
            .then_with(|| a.sort_order.cmp(&b.sort_order))
    });
    rows.dedup_by(|later, earlier| later.valid_at == earlier.valid_at);

    // Compute consecutive rain_trace step deltas, handling the 9am local reset:
    // if cur_rain < prev_rain, the counter reset at 9am, so use cur_rain directly.
    let mut step_deltas: Vec<(DateTime<Utc>, f64)> = Vec::with_capacity(rows.len());
    let mut last_rain: Option<(DateTime<Utc>, f64)> = None;

    for row in &rows {
        if let Some(cur_rain) = row.rain_trace {
            if let Some((prev_ts, prev_rain)) = last_rain {
                if row.valid_at - prev_ts <= ChronoDuration::minutes(90) {
                    let delta = if cur_rain < prev_rain {
                        cur_rain.max(0.0)
                    } else {
                        cur_rain - prev_rain
                    };
                    step_deltas.push((row.valid_at, delta));
                }
            }
            last_rain = Some((row.valid_at, cur_rain));
        }
    }

    let mut hourly_points = Vec::with_capacity(rows.len());
    let mut observations = Vec::with_capacity(rows.len());

    for row in &rows {
        let window_start = row.valid_at - ChronoDuration::hours(1);
        let mut window_sum = 0.0;
        let mut has_rain_window = false;
        for &(step_ts, delta) in &step_deltas {
            if step_ts > window_start && step_ts <= row.valid_at {
                window_sum += delta;
                has_rain_window = true;
            }
        }
        let rain_mm_1h = if has_rain_window {
            Some(round_3dp(window_sum))
        } else {
            None
        };

        let mut point = WeatherPoint {
            valid_at: row.valid_at,
            temp_c: row.temp_c,
            feels_like_c: row.feels_like_c,
            feels_like_is_derived: false,
            rain_prob_pct: None,
            rain_mm: rain_mm_1h,
            wind_kmh: row.wind_kmh,
            humidity_pct: row.humidity_pct,
            summary: row.summary.clone(),
        };
        ensure_feels_like(&mut point);

        observations.push(ObservationRecord {
            node_id: node.node_id.clone(),
            observed_at: point.valid_at,
            source_id: SOURCE_ID_OBS_JSON.to_string(),
            quality_rank: 1,
            temp_c: point.temp_c,
            feels_like_c: point.feels_like_c,
            rain_mm_1h,
            rained_bool: rain_mm_1h.map(|r| r >= 0.2),
            wind_kmh: point.wind_kmh,
            humidity_pct: point.humidity_pct,
        });
        hourly_points.push(point);
    }

    let current = hourly_points
        .last()
        .cloned()
        .ok_or_else(|| SourceError::Parse("missing current observation".into()))?;

    let report = SourceReport {
        source_id: SOURCE_ID_OBS_JSON.to_string(),
        display_name: "BOM Station Observations".to_string(),
        kind: SourceKind::Observation,
        fetched_at,
        weight: 1.2,
        current,
        hourly: hourly_points,
        source_url,
    };

    Ok((report, observations))
}

#[async_trait]
impl WeatherSource for BomObsJsonSource {
    fn source_id(&self) -> &'static str {
        SOURCE_ID_OBS_JSON
    }

    fn display_name(&self) -> &'static str {
        "BOM Station Observations"
    }

    fn default_weight(&self) -> f64 {
        1.2
    }

    async fn fetch_mesh(
        &self,
        nodes: &[MeshNode],
        ctx: &FetchContext,
    ) -> Result<SourceOutput, SourceError> {
        let client = build_http_client(ctx)?;
        let fetched_at = Utc::now();
        let mut output = SourceOutput::empty();
        let mut attempted = 0usize;
        let mut last_err: Option<SourceError> = None;

        for node in nodes {
            let (Some(product_id), Some(station_id)) = (
                node.bom_product_id.as_deref(),
                node.bom_station_id.as_deref(),
            ) else {
                continue;
            };
            attempted += 1;
            let url = self.station_url(product_id, station_id);
            debug!(node_id = %node.node_id, %url, "fetching BOM obs JSON");

            let resp = match client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    warn!(node_id = %node.node_id, error = %e, "BOM obs JSON HTTP request failed");
                    last_err = Some(SourceError::Http(e));
                    continue;
                }
            };

            let resp = match resp.error_for_status() {
                Ok(r) => r,
                Err(e) => {
                    warn!(node_id = %node.node_id, error = %e, "BOM obs JSON HTTP error status");
                    last_err = Some(SourceError::Http(e));
                    continue;
                }
            };

            let body = match resp.text().await {
                Ok(t) => t,
                Err(e) => {
                    warn!(node_id = %node.node_id, error = %e, "BOM obs JSON body read failed");
                    last_err = Some(SourceError::Http(e));
                    continue;
                }
            };

            match parse_obs_json(&body, node, fetched_at, Some(url)) {
                Ok((mut report, obs)) => {
                    report.weight = self.default_weight();
                    output.reports.push((node.node_id.clone(), report));
                    output.observations.extend(obs);
                }
                Err(e) => {
                    warn!(node_id = %node.node_id, error = %e, "BOM obs JSON parse failed");
                    last_err = Some(e);
                }
            }
        }

        if attempted > 0 && output.reports.is_empty() {
            if let Some(err) = last_err {
                return Err(err);
            }
        }

        Ok(output)
    }
}

// ---------------------------------------------------------------------------
// 2. BomFtpXmlSource — Anonymous FTP AMOC XML Forecasts
// ---------------------------------------------------------------------------

/// Adapter for Bureau of Meteorology anonymous FTP AMOC XML town/district
/// forecasts under `/anon/gen/fwo/{product}.xml`.
#[derive(Debug, Clone)]
pub struct BomFtpXmlSource {
    host: String,
    base_dir: String,
}

impl Default for BomFtpXmlSource {
    fn default() -> Self {
        Self::new()
    }
}

impl BomFtpXmlSource {
    /// Creates a new `BomFtpXmlSource` targeting `ftp.bom.gov.au:21/anon/gen/fwo`.
    pub fn new() -> Self {
        Self {
            host: DEFAULT_FTP_HOST.to_string(),
            base_dir: DEFAULT_FTP_DIR.to_string(),
        }
    }

    /// Creates a `BomFtpXmlSource` with a custom FTP host and directory.
    pub fn with_endpoint(host: impl Into<String>, base_dir: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            base_dir: base_dir.into(),
        }
    }
}

#[derive(Debug, Clone)]
struct AmocPeriod {
    index: i32,
    valid_at: DateTime<Utc>,
    temp_min_c: Option<f64>,
    temp_max_c: Option<f64>,
    rain_prob_pct: Option<f64>,
    rain_mm: Option<f64>,
    precis: Option<String>,
}

#[derive(Debug, Clone)]
struct AmocArea {
    description: String,
    area_type: String,
    periods: Vec<AmocPeriod>,
}

fn parse_xml_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw.trim())
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn parse_precipitation_range(raw: &str) -> Option<f64> {
    let lower = raw.to_ascii_lowercase();
    let mut nums: Vec<f64> = Vec::new();
    let mut current = String::new();

    for ch in lower.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            current.push(ch);
        } else if !current.is_empty() {
            if let Ok(n) = current.parse::<f64>() {
                nums.push(n);
            }
            current.clear();
        }
    }
    if !current.is_empty() {
        if let Ok(n) = current.parse::<f64>() {
            nums.push(n);
        }
    }

    match nums.as_slice() {
        [low, high, ..] => Some(round_3dp((low + high) / 2.0)),
        [val] => {
            if lower.contains("less than") {
                Some(round_3dp(val * 0.5))
            } else {
                Some(round_3dp(*val))
            }
        }
        [] => None,
    }
}

fn parse_probability_pct(raw: &str) -> Option<f64> {
    let cleaned = raw.trim().trim_end_matches('%').trim();
    cleaned.parse::<f64>().ok()
}

fn normalize_place_name(raw: &str) -> String {
    let mut s = raw.trim().to_ascii_lowercase();
    // Strip parenthetical qualifier such as "(Observatory Hill)" or "(Metro)"
    if let Some(idx) = s.find('(') {
        s.truncate(idx);
    }
    s = s.trim().to_string();
    for suffix in [" airport", " seaway", " metro"] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.trim().to_string();
        }
    }
    if let Some((first, _)) = s.split_once('-') {
        // e.g. "kalgoorlie-boulder" -> keep full form or prefix during matching
        if first == "kalgoorlie" {
            return first.to_string();
        }
    }
    s
}

fn area_matches_node(area_desc: &str, node: &MeshNode) -> Option<u8> {
    let area_raw = area_desc.trim().to_ascii_lowercase();
    let node_raw = node.name.trim().to_ascii_lowercase();
    if area_raw == node_raw {
        return Some(3);
    }
    let area_norm = normalize_place_name(area_desc);
    let node_norm = normalize_place_name(&node.name);
    if !area_norm.is_empty() && area_norm == node_norm {
        return Some(2);
    }
    if !area_norm.is_empty()
        && (node_norm.starts_with(&area_norm) || area_norm.starts_with(&node_norm))
    {
        return Some(1);
    }
    None
}

fn attr_value(
    e: &quick_xml::events::BytesStart<'_>,
    decoder: quick_xml::Decoder,
    key: &[u8],
) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == key {
            if let Ok(val) = attr.decode_and_unescape_value(decoder) {
                return Some(val.into_owned());
            }
        }
    }
    None
}

/// Parses a BOM AMOC XML forecast document into `(node_id, SourceReport)` pairs
/// for matching `MeshNode`s. Pure and deterministic for unit testing without FTP.
pub fn parse_amoc_xml(
    xml: &str,
    nodes: &[MeshNode],
    fetched_at: DateTime<Utc>,
) -> Result<Vec<(String, SourceReport)>, SourceError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut product_identifier: Option<String> = None;
    let mut in_amoc = false;
    let mut in_identifier = false;
    let mut in_forecast = false;

    let mut areas: Vec<AmocArea> = Vec::new();
    let mut current_area: Option<AmocArea> = None;
    let mut current_period: Option<AmocPeriod> = None;
    let mut active_field_type: Option<String> = None;
    let mut active_text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let decoder = reader.decoder();
                match e.name().as_ref() {
                    b"amoc" => in_amoc = true,
                    b"identifier" if in_amoc => {
                        in_identifier = true;
                        active_text.clear();
                    }
                    b"forecast" => in_forecast = true,
                    b"area" if in_forecast => {
                        let area_type =
                            attr_value(&e, decoder, b"type").unwrap_or_default();
                        if area_type.eq_ignore_ascii_case("location")
                            || area_type.eq_ignore_ascii_case("metropolitan")
                        {
                            let description =
                                attr_value(&e, decoder, b"description").unwrap_or_default();
                            current_area = Some(AmocArea {
                                description,
                                area_type: area_type.to_ascii_lowercase(),
                                periods: Vec::new(),
                            });
                        }
                    }
                    b"forecast-period" if current_area.is_some() => {
                        let valid_at = attr_value(&e, decoder, b"start-time-utc")
                            .and_then(|s| parse_xml_timestamp(&s))
                            .or_else(|| {
                                attr_value(&e, decoder, b"start-time-local")
                                    .and_then(|s| parse_xml_timestamp(&s))
                            });
                        let index = attr_value(&e, decoder, b"index")
                            .and_then(|s| s.parse::<i32>().ok())
                            .unwrap_or(0);
                        if let Some(valid_at) = valid_at {
                            current_period = Some(AmocPeriod {
                                index,
                                valid_at,
                                temp_min_c: None,
                                temp_max_c: None,
                                rain_prob_pct: None,
                                rain_mm: None,
                                precis: None,
                            });
                        }
                    }
                    b"element" | b"text" if current_period.is_some() => {
                        active_field_type = attr_value(&e, decoder, b"type");
                        active_text.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                if in_identifier || active_field_type.is_some() {
                    let txt = e
                        .unescape()
                        .map_err(|err| SourceError::Parse(format!("XML text unescape: {err}")))?;
                    active_text.push_str(&txt);
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"amoc" => in_amoc = false,
                b"identifier" if in_identifier => {
                    let trimmed = active_text.trim();
                    if !trimmed.is_empty() {
                        product_identifier = Some(trimmed.to_string());
                    }
                    in_identifier = false;
                    active_text.clear();
                }
                b"forecast" => in_forecast = false,
                b"element" | b"text" if active_field_type.is_some() => {
                    if let (Some(field_type), Some(period)) =
                        (active_field_type.take(), current_period.as_mut())
                    {
                        let val = active_text.trim();
                        match field_type.as_str() {
                            "air_temperature_minimum" => {
                                period.temp_min_c = val.parse::<f64>().ok();
                            }
                            "air_temperature_maximum" => {
                                period.temp_max_c = val.parse::<f64>().ok();
                            }
                            "probability_of_precipitation" => {
                                period.rain_prob_pct = parse_probability_pct(val);
                            }
                            "precipitation_range" => {
                                period.rain_mm = parse_precipitation_range(val);
                            }
                            "precis" if !val.is_empty() => {
                                period.precis = Some(val.to_string());
                            }
                            _ => {}
                        }
                    }
                    active_text.clear();
                }
                b"forecast-period" => {
                    if let (Some(period), Some(area)) =
                        (current_period.take(), current_area.as_mut())
                    {
                        area.periods.push(period);
                    }
                }
                b"area" => {
                    if let Some(area) = current_area.take() {
                        if !area.periods.is_empty() {
                            areas.push(area);
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(err) => {
                return Err(SourceError::Parse(format!(
                    "BOM AMOC XML parse error: {err}"
                )));
            }
            _ => {}
        }
    }

    let source_url = product_identifier
        .as_ref()
        .map(|id| format!("ftp://ftp.bom.gov.au/anon/gen/fwo/{id}.xml"));

    let mut reports = Vec::new();
    for node in nodes {
        if let (Some(xml_prod), Some(node_prod)) = (
            product_identifier.as_deref(),
            node.bom_forecast_product.as_deref(),
        ) {
            if !xml_prod.eq_ignore_ascii_case(node_prod) {
                continue;
            }
        }

        // Select the best matching area for this node:
        // higher name-match score wins, then prefer `location` over `metropolitan`.
        let best_area = areas
            .iter()
            .filter_map(|area| {
                let score = area_matches_node(&area.description, node)?;
                let type_priority = u8::from(area.area_type == "location");
                Some(((score, type_priority, area.periods.len()), area))
            })
            .max_by_key(|(key, _)| *key)
            .map(|(_, area)| area);

        let Some(area) = best_area else {
            continue;
        };

        let mut periods = area.periods.clone();
        periods.sort_by(|a, b| a.valid_at.cmp(&b.valid_at).then_with(|| a.index.cmp(&b.index)));

        let points: Vec<WeatherPoint> = periods
            .into_iter()
            .map(|p| {
                let temp_c = match (p.temp_min_c, p.temp_max_c) {
                    (Some(min), Some(max)) => Some(round_3dp((min + max) / 2.0)),
                    (None, Some(max)) => Some(max),
                    (Some(min), None) => Some(min),
                    (None, None) => None,
                };
                WeatherPoint {
                    valid_at: p.valid_at,
                    temp_c,
                    feels_like_c: None,
                    feels_like_is_derived: false,
                    rain_prob_pct: p.rain_prob_pct,
                    rain_mm: p.rain_mm,
                    wind_kmh: None,
                    humidity_pct: None,
                    summary: p.precis,
                }
            })
            .collect();

        let Some(current) = points.first().cloned() else {
            continue;
        };

        reports.push((
            node.node_id.clone(),
            SourceReport {
                source_id: SOURCE_ID_FTP_XML.to_string(),
                display_name: "BOM Official Forecast (FTP)".to_string(),
                kind: SourceKind::Forecast,
                fetched_at,
                weight: 1.0,
                current,
                hourly: points,
                source_url: source_url.clone(),
            },
        ));
    }

    Ok(reports)
}

fn fetch_ftp_products_blocking(
    host: &str,
    base_dir: &str,
    products: &[String],
    timeout: Duration,
) -> Result<Vec<(String, String)>, SourceError> {
    let addr = host
        .to_socket_addrs()
        .map_err(|e| SourceError::Ftp(format!("DNS resolve failed for {host}: {e}")))?
        .next()
        .ok_or_else(|| SourceError::Ftp(format!("no socket address found for {host}")))?;

    let mut ftp = FtpStream::connect_timeout(addr, timeout)
        .map_err(|e| SourceError::Ftp(format!("FTP connect to {host} failed: {e}")))?;
    let _ = ftp.get_ref().set_read_timeout(Some(timeout));
    let _ = ftp.get_ref().set_write_timeout(Some(timeout));
    ftp.set_mode(Mode::Passive);

    ftp.login("anonymous", "turtleheavyweather@example.com")
        .map_err(|e| SourceError::Ftp(format!("FTP anonymous login failed: {e}")))?;
    ftp.cwd(base_dir)
        .map_err(|e| SourceError::Ftp(format!("FTP cwd {base_dir} failed: {e}")))?;
    ftp.transfer_type(FileType::Binary)
        .map_err(|e| SourceError::Ftp(format!("FTP binary transfer mode failed: {e}")))?;

    let mut files = Vec::new();
    for product in products {
        let filename = format!("{product}.xml");
        match ftp.retr_as_buffer(&filename) {
            Ok(cursor) => {
                let xml = String::from_utf8_lossy(cursor.get_ref()).into_owned();
                files.push((product.clone(), xml));
            }
            Err(e) => {
                warn!(%filename, error = %e, "FTP RETR failed for BOM product");
            }
        }
    }

    let _ = ftp.quit();
    Ok(files)
}

#[async_trait]
impl WeatherSource for BomFtpXmlSource {
    fn source_id(&self) -> &'static str {
        SOURCE_ID_FTP_XML
    }

    fn display_name(&self) -> &'static str {
        "BOM Official Forecast (FTP)"
    }

    fn default_weight(&self) -> f64 {
        1.0
    }

    async fn fetch_mesh(
        &self,
        nodes: &[MeshNode],
        ctx: &FetchContext,
    ) -> Result<SourceOutput, SourceError> {
        let products: Vec<String> = nodes
            .iter()
            .filter_map(|n| n.bom_forecast_product.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        if products.is_empty() {
            return Ok(SourceOutput::empty());
        }

        let host = self.host.clone();
        let base_dir = self.base_dir.clone();
        let timeout = Duration::from_secs(ctx.timeout_secs.max(1));

        let files = tokio::task::spawn_blocking(move || {
            fetch_ftp_products_blocking(&host, &base_dir, &products, timeout)
        })
        .await
        .map_err(|e| SourceError::Ftp(format!("FTP blocking task failed: {e}")))??;

        let fetched_at = Utc::now();
        let mut output = SourceOutput::empty();
        for (product, xml) in files {
            let matching_nodes: Vec<MeshNode> = nodes
                .iter()
                .filter(|n| {
                    n.bom_forecast_product
                        .as_deref()
                        .is_some_and(|p| p.eq_ignore_ascii_case(&product))
                })
                .cloned()
                .collect();
            match parse_amoc_xml(&xml, &matching_nodes, fetched_at) {
                Ok(mut parsed) => {
                    for (_, report) in &mut parsed {
                        report.weight = self.default_weight();
                    }
                    output.reports.extend(parsed);
                }
                Err(e) => {
                    warn!(%product, error = %e, "failed to parse BOM AMOC XML");
                }
            }
        }

        Ok(output)
    }
}

// ---------------------------------------------------------------------------
// 3. BomWebApiSource — Opt-in / Circuit-Broken Unofficial BOM App JSON API
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct CircuitBreakerState {
    failure_threshold: u32,
    cooldown: Duration,
    consecutive_failures: u32,
    opened_at: Option<Instant>,
}

impl CircuitBreakerState {
    fn new(failure_threshold: u32, cooldown: Duration) -> Self {
        Self {
            failure_threshold: failure_threshold.max(1),
            cooldown,
            consecutive_failures: 0,
            opened_at: None,
        }
    }

    fn is_open(&mut self) -> bool {
        if let Some(opened) = self.opened_at {
            if opened.elapsed() < self.cooldown {
                return true;
            }
            // Cooldown elapsed: allow half-open probe
            self.opened_at = None;
            self.consecutive_failures = 0;
        }
        false
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.opened_at = None;
    }

    fn record_failure(&mut self) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= self.failure_threshold {
            self.opened_at = Some(Instant::now());
        }
    }
}

/// Opt-in, circuit-broken adapter for the unofficial BOM mobile/web JSON API
/// (`https://api.weather.bom.gov.au/v1/`).
#[derive(Debug, Clone)]
pub struct BomWebApiSource {
    base_url: String,
    base_url_overridden: bool,
    circuit: Arc<Mutex<CircuitBreakerState>>,
    geohash_cache: Arc<Mutex<HashMap<String, String>>>,
}

impl Default for BomWebApiSource {
    fn default() -> Self {
        Self::new()
    }
}

impl BomWebApiSource {
    /// Creates a new `BomWebApiSource` targeting `https://api.weather.bom.gov.au`.
    /// Disabled by default unless `THW_ENABLE_BOM_WEB_API=1`.
    pub fn new() -> Self {
        Self {
            base_url: DEFAULT_WEB_API_BASE_URL.to_string(),
            base_url_overridden: false,
            circuit: Arc::new(Mutex::new(CircuitBreakerState::new(
                3,
                Duration::from_secs(300),
            ))),
            geohash_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Creates a `BomWebApiSource` with a custom base URL (automatically enabled
    /// for `wiremock` and integration tests).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            base_url_overridden: true,
            circuit: Arc::new(Mutex::new(CircuitBreakerState::new(
                3,
                Duration::from_secs(300),
            ))),
            geohash_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Configures the circuit breaker failure threshold and cooldown window.
    pub fn with_circuit_breaker(mut self, failure_threshold: u32, cooldown: Duration) -> Self {
        self.circuit = Arc::new(Mutex::new(CircuitBreakerState::new(
            failure_threshold,
            cooldown,
        )));
        self
    }

    /// Returns whether the circuit breaker is currently open.
    pub fn is_circuit_open(&self) -> bool {
        self.circuit
            .lock()
            .map(|mut guard| guard.is_open())
            .unwrap_or(false)
    }

    fn record_success(&self) {
        if let Ok(mut guard) = self.circuit.lock() {
            guard.record_success();
        }
    }

    fn record_failure(&self) {
        if let Ok(mut guard) = self.circuit.lock() {
            guard.record_failure();
        }
    }
}

#[derive(Debug, Deserialize)]
struct BomWebLocationsResponse {
    data: Vec<BomWebLocationEntry>,
}

#[derive(Debug, Deserialize)]
struct BomWebLocationEntry {
    geohash: String,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BomWebHourlyResponse {
    data: Vec<BomWebHourlyEntry>,
}

#[derive(Debug, Deserialize)]
struct BomWebHourlyEntry {
    time: String,
    #[serde(default)]
    temp: Option<f64>,
    #[serde(default)]
    temp_feels_like: Option<f64>,
    #[serde(default)]
    rain: Option<BomWebRain>,
    #[serde(default)]
    relative_humidity: Option<f64>,
    #[serde(default)]
    wind: Option<BomWebWind>,
    #[serde(default)]
    short_text: Option<String>,
    #[serde(default)]
    icon_descriptor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BomWebRain {
    #[serde(default)]
    amount: Option<BomWebRainAmount>,
    #[serde(default)]
    chance: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct BomWebRainAmount {
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct BomWebWind {
    #[serde(default)]
    speed_kilometre: Option<f64>,
}

fn state_code_str(state: &StateCode) -> &'static str {
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

fn parse_bom_web_hourly(
    json_str: &str,
    fetched_at: DateTime<Utc>,
    source_url: String,
    weight: f64,
) -> Result<SourceReport, SourceError> {
    let payload: BomWebHourlyResponse = serde_json::from_str(json_str)
        .map_err(|e| SourceError::Parse(format!("BOM web hourly JSON parse error: {e}")))?;

    let mut points: Vec<WeatherPoint> = payload
        .data
        .into_iter()
        .filter_map(|entry| {
            let valid_at = parse_xml_timestamp(&entry.time)?;
            let rain_mm = entry.rain.as_ref().and_then(|r| {
                r.amount.as_ref().and_then(|amt| match (amt.min, amt.max) {
                    (Some(min), Some(max)) => Some(round_3dp((min + max) / 2.0)),
                    (Some(min), None) => Some(min),
                    (None, Some(max)) => Some(max),
                    (None, None) => None,
                })
            });
            let summary = clean_bom_text(entry.short_text).or_else(|| {
                clean_bom_text(entry.icon_descriptor).map(|icon| icon.replace('_', " "))
            });
            let mut point = WeatherPoint {
                valid_at,
                temp_c: entry.temp,
                feels_like_c: entry.temp_feels_like,
                feels_like_is_derived: false,
                rain_prob_pct: entry.rain.and_then(|r| r.chance),
                rain_mm,
                wind_kmh: entry.wind.and_then(|w| w.speed_kilometre),
                humidity_pct: entry.relative_humidity,
                summary,
            };
            ensure_feels_like(&mut point);
            Some(point)
        })
        .collect();

    if points.is_empty() {
        return Err(SourceError::Parse(
            "BOM web hourly response contained no valid entries".into(),
        ));
    }

    points.sort_by_key(|p| p.valid_at);
    let current = points[0].clone();

    Ok(SourceReport {
        source_id: SOURCE_ID_WEB_API.to_string(),
        display_name: "BOM Weather App API".to_string(),
        kind: SourceKind::Forecast,
        fetched_at,
        weight,
        current,
        hourly: points,
        source_url: Some(source_url),
    })
}

async fn resolve_geohash(
    client: &Client,
    base_url: &str,
    node: &MeshNode,
    cache: &Arc<Mutex<HashMap<String, String>>>,
) -> Result<String, SourceError> {
    if let Ok(guard) = cache.lock() {
        if let Some(geohash) = guard.get(&node.node_id) {
            return Ok(geohash.clone());
        }
    }

    let query = normalize_place_name(&node.name);
    let search_term = if query.is_empty() {
        node.name.as_str()
    } else {
        query.as_str()
    };

    let resp = client
        .get(format!("{base_url}/v1/locations"))
        .query(&[("search", search_term)])
        .send()
        .await?
        .error_for_status()?;

    let body = resp.text().await?;
    let parsed: BomWebLocationsResponse = serde_json::from_str(&body)
        .map_err(|e| SourceError::Parse(format!("BOM web locations JSON parse error: {e}")))?;

    let expected_state = state_code_str(&node.state);
    let entry = parsed
        .data
        .iter()
        .find(|e| {
            e.state
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case(expected_state))
        })
        .or_else(|| parsed.data.first())
        .ok_or_else(|| {
            SourceError::Parse(format!("BOM web locations empty for {}", node.name))
        })?;

    let geohash = entry.geohash.clone();
    if let Ok(mut guard) = cache.lock() {
        guard.insert(node.node_id.clone(), geohash.clone());
    }
    Ok(geohash)
}

#[async_trait]
impl WeatherSource for BomWebApiSource {
    fn source_id(&self) -> &'static str {
        SOURCE_ID_WEB_API
    }

    fn display_name(&self) -> &'static str {
        "BOM Weather App API"
    }

    fn default_weight(&self) -> f64 {
        0.9
    }

    fn is_enabled(&self) -> bool {
        if self.base_url_overridden {
            return true;
        }
        std::env::var("THW_ENABLE_BOM_WEB_API")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    }

    async fn fetch_mesh(
        &self,
        nodes: &[MeshNode],
        ctx: &FetchContext,
    ) -> Result<SourceOutput, SourceError> {
        if !self.is_enabled() {
            return Err(SourceError::Disabled(
                "THW_ENABLE_BOM_WEB_API is not enabled".to_string(),
            ));
        }

        if self.is_circuit_open() {
            warn!("BOM web API circuit breaker is open; skipping fetch");
            return Ok(SourceOutput::empty());
        }

        let client = build_http_client(ctx)?;
        let fetched_at = Utc::now();
        let mut output = SourceOutput::empty();

        for node in nodes {
            if self.is_circuit_open() {
                warn!("BOM web API circuit breaker opened mid-fetch; stopping remaining nodes");
                break;
            }

            let geohash = match resolve_geohash(
                &client,
                &self.base_url,
                node,
                &self.geohash_cache,
            )
            .await
            {
                Ok(gh) => gh,
                Err(e) => {
                    warn!(node_id = %node.node_id, error = %e, "BOM web API location lookup failed");
                    self.record_failure();
                    continue;
                }
            };

            let forecast_url = format!("{}/v1/locations/{}/forecasts/hourly", self.base_url, geohash);
            let resp = match client.get(&forecast_url).send().await {
                Ok(r) => match r.error_for_status() {
                    Ok(ok) => ok,
                    Err(e) => {
                        warn!(node_id = %node.node_id, error = %e, "BOM web API forecast HTTP status error");
                        self.record_failure();
                        continue;
                    }
                },
                Err(e) => {
                    warn!(node_id = %node.node_id, error = %e, "BOM web API forecast request failed");
                    self.record_failure();
                    continue;
                }
            };

            let body = match resp.text().await {
                Ok(t) => t,
                Err(e) => {
                    warn!(node_id = %node.node_id, error = %e, "BOM web API forecast body read failed");
                    self.record_failure();
                    continue;
                }
            };

            match parse_bom_web_hourly(&body, fetched_at, forecast_url, self.default_weight()) {
                Ok(report) => {
                    self.record_success();
                    output.reports.push((node.node_id.clone(), report));
                }
                Err(e) => {
                    warn!(node_id = %node.node_id, error = %e, "BOM web API forecast schema/parse error");
                    self.record_failure();
                }
            }
        }

        Ok(output)
    }
}
