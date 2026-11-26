use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;
use thw_core::{
    ensure_feels_like, MeshNode, ObservationRecord, SourceKind, SourceReport, WeatherPoint,
};

use crate::{FetchContext, SourceError, SourceOutput, WeatherSource};

pub const DEFAULT_OPEN_METEO_BASE_URL: &str = "https://api.open-meteo.com";
pub const DEFAULT_OPEN_METEO_BATCH_SIZE: usize = 100;

const HOURLY_FIELDS: &str = "temperature_2m,apparent_temperature,precipitation_probability,precipitation,wind_speed_10m,relative_humidity_2m,shortwave_radiation";

/// Meteorological NWP model exposed through Open-Meteo's `/v1/forecast` API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpenMeteoModel {
    BomAccessGlobal,
    EcmwfIfs,
    GfsGlobal,
    IconGlobal,
}

impl OpenMeteoModel {
    /// Distinct `source_id` used in reports, persistence, and regression tables.
    pub fn source_id(self) -> &'static str {
        match self {
            Self::BomAccessGlobal => "open_meteo_bom_access",
            Self::EcmwfIfs => "open_meteo_ecmwf",
            Self::GfsGlobal => "open_meteo_gfs",
            Self::IconGlobal => "open_meteo_icon",
        }
    }

    /// Human-readable display name for UI presentation.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::BomAccessGlobal => "Open-Meteo (BOM ACCESS-G)",
            Self::EcmwfIfs => "Open-Meteo (ECMWF IFS)",
            Self::GfsGlobal => "Open-Meteo (NOAA GFS)",
            Self::IconGlobal => "Open-Meteo (DWD ICON)",
        }
    }

    /// Query value passed to Open-Meteo's `models=` parameter.
    pub fn api_model(self) -> &'static str {
        match self {
            Self::BomAccessGlobal => "bom_access_global",
            Self::EcmwfIfs => "ecmwf_ifs025",
            Self::GfsGlobal => "gfs_global",
            Self::IconGlobal => "icon_seamless",
        }
    }

    /// Initial prior weight for multi-source blending before regression tuning.
    pub fn default_weight(self) -> f64 {
        match self {
            Self::BomAccessGlobal => 1.15,
            Self::EcmwfIfs => 1.10,
            Self::GfsGlobal => 0.95,
            Self::IconGlobal => 1.00,
        }
    }
}

/// WeatherSource adapter for Open-Meteo Forecast API with multi-coordinate batching.
#[derive(Debug, Clone)]
pub struct OpenMeteoSource {
    model: OpenMeteoModel,
    base_url: String,
    batch_size: usize,
    enabled: bool,
    emit_tier2_observations: bool,
}

impl OpenMeteoSource {
    /// Creates an enabled `OpenMeteoSource` for the specified model using default base URL and batch size (50).
    pub fn new(model: OpenMeteoModel) -> Self {
        Self {
            model,
            base_url: DEFAULT_OPEN_METEO_BASE_URL.to_string(),
            batch_size: DEFAULT_OPEN_METEO_BATCH_SIZE,
            enabled: true,
            emit_tier2_observations: false,
        }
    }

    /// Convenience constructor for Open-Meteo BOM ACCESS-G.
    pub fn bom_access() -> Self {
        Self::new(OpenMeteoModel::BomAccessGlobal)
    }

    /// Convenience constructor for Open-Meteo ECMWF IFS.
    pub fn ecmwf() -> Self {
        Self::new(OpenMeteoModel::EcmwfIfs)
    }

    /// Convenience constructor for Open-Meteo NOAA GFS.
    pub fn gfs() -> Self {
        Self::new(OpenMeteoModel::GfsGlobal)
    }

    /// Overrides the API base URL (useful for wiremock tests).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }

    /// Overrides the coordinate batch size per HTTP request (clamped to >= 1).
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size.max(1);
        self
    }

    /// Enables or disables this source adapter.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Enables emitting Tier-2 (`quality_rank = 2`) fallback observations from the nearest hour.
    pub fn with_tier2_observations(mut self, emit: bool) -> Self {
        self.emit_tier2_observations = emit;
        self
    }

    pub fn model(&self) -> OpenMeteoModel {
        self.model
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn batch_size(&self) -> usize {
        self.batch_size
    }

    fn build_batch_url(&self, nodes: &[MeshNode]) -> String {
        let lats = nodes
            .iter()
            .map(|n| n.lat.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let lons = nodes
            .iter()
            .map(|n| n.lon.to_string())
            .collect::<Vec<_>>()
            .join(",");

        format!(
            "{}/v1/forecast?latitude={}&longitude={}&hourly={}&models={}&timezone=UTC",
            self.base_url,
            lats,
            lons,
            HOURLY_FIELDS,
            self.model.api_model()
        )
    }
}

#[async_trait]
impl WeatherSource for OpenMeteoSource {
    fn source_id(&self) -> &'static str {
        self.model.source_id()
    }

    fn display_name(&self) -> &'static str {
        self.model.display_name()
    }

    fn default_weight(&self) -> f64 {
        self.model.default_weight()
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

        let client = reqwest::Client::builder()
            .user_agent(&ctx.user_agent)
            .timeout(Duration::from_secs(ctx.timeout_secs))
            .build()?;

        let mut output = SourceOutput::empty();
        let fetched_at = Utc::now();

        for chunk in nodes.chunks(self.batch_size.max(1)) {
            let url = self.build_batch_url(chunk);
            let resp = client.get(&url).send().await?;

            if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(SourceError::RateLimited(self.source_id().to_string()));
            }
            let resp = resp.error_for_status()?;
            let body = resp.text().await?;

            let parsed: OpenMeteoResponse = serde_json::from_str(&body).map_err(|e| {
                SourceError::Parse(format!("failed to parse Open-Meteo response: {e}"))
            })?;

            let locations = parsed.into_vec();
            if locations.len() != chunk.len() {
                tracing::warn!(
                    source_id = self.source_id(),
                    expected = chunk.len(),
                    received = locations.len(),
                    "Open-Meteo batch response count mismatch"
                );
            }

            for (node, loc) in chunk.iter().zip(locations) {
                let hourly = parse_hourly_points(&loc.hourly);
                let Some(current) = select_nearest_point(&hourly, fetched_at) else {
                    tracing::warn!(
                        source_id = self.source_id(),
                        node_id = %node.node_id,
                        "Open-Meteo returned no valid hourly timesteps for node"
                    );
                    continue;
                };

                if self.emit_tier2_observations {
                    output.observations.push(ObservationRecord {
                        node_id: node.node_id.clone(),
                        observed_at: current.valid_at,
                        source_id: self.source_id().to_string(),
                        quality_rank: 2,
                        temp_c: current.temp_c,
                        feels_like_c: current.feels_like_c,
                        rain_mm_1h: current.rain_mm,
                        rained_bool: current.rain_mm.map(|mm| mm > 0.0),
                        wind_kmh: current.wind_kmh,
                        humidity_pct: current.humidity_pct,
                    });
                }

                output.reports.push((
                    node.node_id.clone(),
                    SourceReport {
                        source_id: self.source_id().to_string(),
                        display_name: self.display_name().to_string(),
                        kind: SourceKind::Forecast,
                        fetched_at,
                        weight: self.default_weight(),
                        current,
                        hourly,
                        source_url: Some(url.clone()),
                    },
                ));
            }
        }

        Ok(output)
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OpenMeteoResponse {
    Many(Vec<OpenMeteoLocation>),
    One(OpenMeteoLocation),
}

impl OpenMeteoResponse {
    fn into_vec(self) -> Vec<OpenMeteoLocation> {
        match self {
            Self::Many(v) => v,
            Self::One(one) => vec![one],
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpenMeteoLocation {
    #[serde(default)]
    hourly: OpenMeteoHourly,
}

#[derive(Debug, Default, Deserialize)]
struct OpenMeteoHourly {
    #[serde(default)]
    time: Vec<String>,
    #[serde(default)]
    temperature_2m: Vec<Option<f64>>,
    #[serde(default)]
    apparent_temperature: Vec<Option<f64>>,
    #[serde(default)]
    precipitation_probability: Vec<Option<f64>>,
    #[serde(default)]
    precipitation: Vec<Option<f64>>,
    #[serde(default)]
    wind_speed_10m: Vec<Option<f64>>,
    #[serde(default)]
    relative_humidity_2m: Vec<Option<f64>>,
    #[serde(default)]
    shortwave_radiation: Vec<Option<f64>>,
}

fn parse_hourly_points(hourly: &OpenMeteoHourly) -> Vec<WeatherPoint> {
    let mut points = Vec::with_capacity(hourly.time.len());

    for (idx, ts) in hourly.time.iter().enumerate() {
        let Some(valid_at) = parse_open_meteo_time(ts) else {
            tracing::warn!(timestamp = %ts, "skipping unparseable Open-Meteo timestamp");
            continue;
        };

        let mut point = WeatherPoint {
            valid_at,
            temp_c: hourly.temperature_2m.get(idx).copied().flatten(),
            feels_like_c: hourly.apparent_temperature.get(idx).copied().flatten(),
            feels_like_is_derived: false,
            rain_prob_pct: hourly.precipitation_probability.get(idx).copied().flatten(),
            rain_mm: hourly.precipitation.get(idx).copied().flatten(),
            wind_kmh: hourly.wind_speed_10m.get(idx).copied().flatten(),
            humidity_pct: hourly.relative_humidity_2m.get(idx).copied().flatten(),
            solar_wm2: hourly.shortwave_radiation.get(idx).copied().flatten(),
            summary: None,
        };
        ensure_feels_like(&mut point);
        if point.temp_c.is_none()
            && point.feels_like_c.is_none()
            && point.rain_prob_pct.is_none()
            && point.rain_mm.is_none()
        {
            continue;
        }
        points.push(point);
    }

    points
}

fn parse_open_meteo_time(raw: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M") {
        return Some(ndt.and_utc());
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S") {
        return Some(ndt.and_utc());
    }
    None
}

fn select_nearest_point(points: &[WeatherPoint], target: DateTime<Utc>) -> Option<WeatherPoint> {
    points
        .iter()
        .min_by_key(|p| (p.valid_at - target).num_seconds().abs())
        .cloned()
}
