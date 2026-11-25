use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum StateCode {
    NSW,
    VIC,
    QLD,
    SA,
    WA,
    TAS,
    NT,
    ACT,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshNode {
    pub node_id: String,
    pub name: String,
    pub state: StateCode,
    pub lat: f64,
    pub lon: f64,
    pub bom_station_id: Option<String>,
    pub bom_product_id: Option<String>,
    pub bom_forecast_product: Option<String>,
    #[serde(default)]
    pub is_major_hub: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocationIndexEntry {
    pub suburb: String,
    pub postcode: String,
    pub state: StateCode,
    pub lat: f64,
    pub lon: f64,
    pub node_id: String,
    pub distance_km: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Observation,
    Forecast,
    Hybrid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeatherPoint {
    pub valid_at: DateTime<Utc>,
    pub temp_c: Option<f64>,
    pub feels_like_c: Option<f64>,
    #[serde(default)]
    pub feels_like_is_derived: bool,
    pub rain_prob_pct: Option<f64>,
    pub rain_mm: Option<f64>,
    pub wind_kmh: Option<f64>,
    pub humidity_pct: Option<f64>,
    #[serde(default)]
    pub solar_wm2: Option<f64>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceReport {
    pub source_id: String,
    pub display_name: String,
    pub kind: SourceKind,
    pub fetched_at: DateTime<Utc>,
    pub weight: f64,
    pub current: WeatherPoint,
    #[serde(default)]
    pub hourly: Vec<WeatherPoint>,
    #[serde(default)]
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RainVerdict {
    Dry,
    Possible,
    Likely,
    Raining,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RainSummary {
    pub verdict: RainVerdict,
    pub label: String,
    pub probability_pct: f64,
    pub expected_mm: f64,
    pub is_raining_now: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLevel {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TempSpread {
    pub feels_like_min_c: f64,
    pub feels_like_max_c: f64,
    pub temp_min_c: f64,
    pub temp_max_c: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BestEstimate {
    pub valid_at: DateTime<Utc>,
    pub feels_like_c: f64,
    #[serde(default)]
    pub feels_like_sun_c: Option<f64>,
    pub temp_c: f64,
    pub rain: RainSummary,
    pub confidence: ConfidenceLevel,
    pub spread: TempSpread,
    pub source_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HourlyEstimate {
    pub valid_at: DateTime<Utc>,
    pub feels_like_c: f64,
    #[serde(default)]
    pub feels_like_sun_c: Option<f64>,
    pub temp_c: f64,
    pub rain_prob_pct: f64,
    pub rain_mm_expected: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeWeatherBundle {
    pub generated_at: DateTime<Utc>,
    pub node: MeshNode,
    pub current_best_estimate: BestEstimate,
    #[serde(default)]
    pub hourly_best_estimate: Vec<HourlyEstimate>,
    #[serde(default)]
    pub sources: Vec<SourceReport>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservationRecord {
    pub node_id: String,
    pub observed_at: DateTime<Utc>,
    pub source_id: String,
    pub quality_rank: u8,
    pub temp_c: Option<f64>,
    pub feels_like_c: Option<f64>,
    pub rain_mm_1h: Option<f64>,
    pub rained_bool: Option<bool>,
    pub wind_kmh: Option<f64>,
    pub humidity_pct: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegressionMetricRow {
    pub computed_at: DateTime<Utc>,
    pub window_days: u32,
    pub subject_id: String,
    pub display_name: String,
    pub lead_bucket: String,
    pub sample_count: usize,
    pub temp_mae_c: Option<f64>,
    pub temp_rmse_c: Option<f64>,
    pub temp_bias_c: Option<f64>,
    pub feels_like_mae_c: Option<f64>,
    pub feels_like_rmse_c: Option<f64>,
    pub rain_brier_score: Option<f64>,
    pub rain_hit_rate: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegressionSummary {
    pub computed_at: DateTime<Utc>,
    pub window_days: u32,
    pub metrics: Vec<RegressionMetricRow>,
    #[serde(default)]
    pub suggested_weights: HashMap<String, f64>,
}
