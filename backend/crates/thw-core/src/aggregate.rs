use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Duration, Timelike, Utc};

use crate::feels_like::{compute_outdoor_sun_apparent_temp_c, normalize_feels_like_bom_steadman};
use crate::models::{
    BestEstimate, ConfidenceLevel, HourlyEstimate, MeshNode, NodeWeatherBundle, RainSummary,
    RainVerdict, SourceKind, SourceReport, TempSpread, UvSummary, WeatherPoint,
};

#[derive(Debug, Clone)]
pub struct AggregationConfig {
    pub source_weights: HashMap<String, f64>,
    pub max_obs_age_hours: i64,
    pub max_forecast_age_hours: i64,
    pub hourly_horizon_hours: usize,
}

impl Default for AggregationConfig {
    fn default() -> Self {
        let mut source_weights = HashMap::new();
        source_weights.insert("bom_obs_json".to_string(), 1.5);
        source_weights.insert("bom_ftp_obs".to_string(), 1.5);
        source_weights.insert("bom_ftp_xml".to_string(), 1.3);
        source_weights.insert("bom_web_api".to_string(), 1.25);
        source_weights.insert("open_meteo_bom_access".to_string(), 1.2);
        source_weights.insert("open_meteo_ecmwf".to_string(), 1.15);
        source_weights.insert("open_meteo_ukmo".to_string(), 1.1);
        source_weights.insert("open_meteo_ecmwf_aifs".to_string(), 1.05);
        source_weights.insert("open_meteo_icon".to_string(), 1.05);
        source_weights.insert("met_no".to_string(), 1.05);
        source_weights.insert("open_meteo_gfs".to_string(), 0.95);
        source_weights.insert("weatherapi".to_string(), 0.95);
        source_weights.insert("pirate_weather".to_string(), 0.9);
        source_weights.insert("openweathermap".to_string(), 0.85);
        source_weights.insert("tomorrow_io".to_string(), 0.95);
        source_weights.insert("bom_html_scraper".to_string(), 0.85);

        Self {
            source_weights,
            max_obs_age_hours: 4,
            max_forecast_age_hours: 18,
            hourly_horizon_hours: 168,
        }
    }
}

impl AggregationConfig {
    pub fn weight_for(&self, report: &SourceReport) -> f64 {
        self.source_weights
            .get(&report.source_id)
            .copied()
            .unwrap_or(report.weight)
            .max(0.05)
    }
}

fn is_fresh(report: &SourceReport, now: DateTime<Utc>, cfg: &AggregationConfig) -> bool {
    let age = now.signed_duration_since(report.fetched_at);
    let max_age = match report.kind {
        SourceKind::Observation => Duration::hours(cfg.max_obs_age_hours),
        SourceKind::Forecast | SourceKind::Hybrid => Duration::hours(cfg.max_forecast_age_hours),
    };
    age <= max_age
}

fn weighted_mean<I>(items: I) -> Option<f64>
where
    I: Iterator<Item = (f64, f64)>,
{
    let mut sum = 0.0;
    let mut w_sum = 0.0;
    for (val, w) in items {
        if val.is_finite() && w > 0.0 {
            sum += val * w;
            w_sum += w;
        }
    }
    if w_sum > 0.0 {
        Some((sum / w_sum * 10.0).round() / 10.0)
    } else {
        None
    }
}

pub fn classify_uv_category(index: f64) -> &'static str {
    if index >= 11.0 {
        "Extreme"
    } else if index >= 8.0 {
        "Very High"
    } else if index >= 6.0 {
        "High"
    } else if index >= 3.0 {
        "Moderate"
    } else {
        "Low"
    }
}

fn parse_uv_summary_from_report(report: &SourceReport) -> Option<UvSummary> {
    let summary = report.current.summary.as_deref()?;
    // Expected format: "UV 5.3 (Moderate) · Melbourne"
    let rest = summary.strip_prefix("UV ")?;
    let (idx_str, after_idx) = rest.split_once(' ')?;
    let index: f64 = idx_str.parse().ok()?;
    let station_name = after_idx
        .split_once("· ")
        .map(|(_, s)| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "ARPANSA".to_string());
    Some(UvSummary {
        index,
        category: classify_uv_category(index).to_string(),
        station_name,
        observed_at: report.current.valid_at,
    })
}

pub fn classify_rain(prob_pct: f64, expected_mm: f64, is_raining_now: bool) -> RainSummary {
    let (verdict, label) = if is_raining_now {
        (RainVerdict::Raining, "Raining now".to_string())
    } else if prob_pct >= 60.0 || expected_mm >= 1.0 {
        (RainVerdict::Likely, "Rain likely".to_string())
    } else if prob_pct >= 25.0 || expected_mm >= 0.2 {
        (RainVerdict::Possible, "Possible showers".to_string())
    } else {
        (RainVerdict::Dry, "Mostly dry".to_string())
    };

    RainSummary {
        verdict,
        label,
        probability_pct: prob_pct.clamp(0.0, 100.0).round(),
        expected_mm: (expected_mm.max(0.0) * 10.0).round() / 10.0,
        is_raining_now,
    }
}

fn truncate_to_hour(dt: DateTime<Utc>) -> DateTime<Utc> {
    dt.with_minute(0)
        .and_then(|d| d.with_second(0))
        .and_then(|d| d.with_nanosecond(0))
        .unwrap_or(dt)
}

pub fn aggregate_node_bundle(
    node: &MeshNode,
    reports: &[SourceReport],
    now: DateTime<Utc>,
    cfg: &AggregationConfig,
) -> Option<NodeWeatherBundle> {
    let mut normalized_reports: Vec<SourceReport> = reports.to_vec();
    for r in &mut normalized_reports {
        if r.kind != SourceKind::Observation {
            normalize_feels_like_bom_steadman(&mut r.current);
            for pt in &mut r.hourly {
                normalize_feels_like_bom_steadman(pt);
            }
        }
    }

    let fresh: Vec<&SourceReport> = normalized_reports
        .iter()
        .filter(|r| is_fresh(r, now, cfg))
        .collect();
    let active: Vec<&SourceReport> = if fresh.is_empty() {
        normalized_reports.iter().collect()
    } else {
        fresh
    };

    if active.is_empty() {
        return None;
    }

    let temps: Vec<(f64, f64)> = active
        .iter()
        .filter_map(|r| r.current.temp_c.map(|t| (t, cfg.weight_for(r))))
        .collect();
    let temp_c = weighted_mean(temps.iter().copied())?;

    let feels: Vec<(f64, f64)> = active
        .iter()
        .filter_map(|r| {
            r.current
                .feels_like_c
                .or(r.current.temp_c)
                .map(|f| (f, cfg.weight_for(r)))
        })
        .collect();
    let feels_like_c = weighted_mean(feels.iter().copied()).unwrap_or(temp_c);

    let rain_probs: Vec<(f64, f64)> = active
        .iter()
        .filter_map(|r| r.current.rain_prob_pct.map(|p| (p, cfg.weight_for(r))))
        .collect();
    let rain_prob_pct = weighted_mean(rain_probs.into_iter()).unwrap_or(0.0);

    let rain_mms: Vec<(f64, f64)> = active
        .iter()
        .filter_map(|r| r.current.rain_mm.map(|mm| (mm, cfg.weight_for(r))))
        .collect();
    let expected_mm = weighted_mean(rain_mms.into_iter()).unwrap_or(0.0);

    let is_raining_now = active
        .iter()
        .any(|r| r.kind == SourceKind::Observation && r.current.rain_mm.unwrap_or(0.0) >= 0.2);

    let temp_min_c = temps.iter().map(|(v, _)| *v).fold(f64::INFINITY, f64::min);
    let temp_max_c = temps
        .iter()
        .map(|(v, _)| *v)
        .fold(f64::NEG_INFINITY, f64::max);
    let feels_like_min_c = feels.iter().map(|(v, _)| *v).fold(f64::INFINITY, f64::min);
    let feels_like_max_c = feels
        .iter()
        .map(|(v, _)| *v)
        .fold(f64::NEG_INFINITY, f64::max);

    let weather_source_count = active
        .iter()
        .filter(|r| r.current.temp_c.is_some() || r.current.feels_like_c.is_some())
        .count();

    let temp_spread = (temp_max_c - temp_min_c).max(0.0);
    let confidence = if weather_source_count >= 3 && temp_spread <= 3.0 {
        ConfidenceLevel::High
    } else if weather_source_count >= 2 && temp_spread <= 5.0 {
        ConfidenceLevel::Medium
    } else {
        ConfidenceLevel::Low
    };

    let solar_wm2 = weighted_mean(
        active
            .iter()
            .filter_map(|r| r.current.solar_wm2.map(|s| (s, cfg.weight_for(r)))),
    );
    let feels_like_sun_c = solar_wm2
        .filter(|s| *s >= 50.0)
        .map(|s| compute_outdoor_sun_apparent_temp_c(feels_like_c, s));

    let uv = active
        .iter()
        .find(|r| r.source_id == "arpansa_uv")
        .and_then(|r| parse_uv_summary_from_report(r));

    let current_best_estimate = BestEstimate {
        valid_at: now,
        feels_like_c,
        feels_like_sun_c,
        temp_c,
        rain: classify_rain(rain_prob_pct, expected_mm, is_raining_now),
        uv,
        confidence,
        spread: TempSpread {
            feels_like_min_c,
            feels_like_max_c,
            temp_min_c,
            temp_max_c,
        },
        source_count: weather_source_count.max(active.len().min(1)),
    };

    let mut hourly_buckets: BTreeMap<DateTime<Utc>, Vec<(&WeatherPoint, f64)>> = BTreeMap::new();
    for report in &active {
        let w = cfg.weight_for(report);
        for pt in &report.hourly {
            if pt.valid_at >= now - Duration::hours(1) {
                hourly_buckets
                    .entry(truncate_to_hour(pt.valid_at))
                    .or_default()
                    .push((pt, w));
            }
        }
    }

    let hourly_best_estimate = hourly_buckets
        .into_iter()
        .take(cfg.hourly_horizon_hours)
        .filter_map(|(valid_at, pts)| {
            let t = weighted_mean(pts.iter().filter_map(|(p, w)| p.temp_c.map(|v| (v, *w))))?;
            let fl = weighted_mean(
                pts.iter()
                    .filter_map(|(p, w)| p.feels_like_c.or(p.temp_c).map(|v| (v, *w))),
            )
            .unwrap_or(t);
            let rp = weighted_mean(
                pts.iter()
                    .filter_map(|(p, w)| p.rain_prob_pct.map(|v| (v, *w))),
            )
            .unwrap_or(0.0);
            let rmm = weighted_mean(pts.iter().filter_map(|(p, w)| p.rain_mm.map(|v| (v, *w))))
                .unwrap_or(0.0);
            let h_solar =
                weighted_mean(pts.iter().filter_map(|(p, w)| p.solar_wm2.map(|v| (v, *w))));
            let h_sun_fl = h_solar
                .filter(|s| *s >= 50.0)
                .map(|s| compute_outdoor_sun_apparent_temp_c(fl, s));

            Some(HourlyEstimate {
                valid_at,
                feels_like_c: fl,
                feels_like_sun_c: h_sun_fl,
                temp_c: t,
                rain_prob_pct: rp.clamp(0.0, 100.0).round(),
                rain_mm_expected: (rmm.max(0.0) * 10.0).round() / 10.0,
            })
        })
        .collect();

    let mut sorted_sources: Vec<SourceReport> = active.into_iter().cloned().collect();
    sorted_sources.sort_by(|a, b| {
        cfg.weight_for(b)
            .total_cmp(&cfg.weight_for(a))
            .then_with(|| a.display_name.cmp(&b.display_name))
    });

    Some(NodeWeatherBundle {
        generated_at: now,
        node: node.clone(),
        current_best_estimate,
        hourly_best_estimate,
        sources: sorted_sources,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::StateCode;

    fn test_node() -> MeshNode {
        MeshNode {
            node_id: "bom-066062".into(),
            name: "Sydney".into(),
            state: StateCode::NSW,
            lat: -33.8607,
            lon: 151.205,
            bom_station_id: Some("94768".into()),
            bom_product_id: Some("IDN60901".into()),
            bom_forecast_product: Some("IDN11060".into()),
            is_major_hub: true,
        }
    }

    #[test]
    fn aggregates_weighted_consensus_and_rain_verdicts() {
        let now = Utc::now();
        let reports = vec![
            SourceReport {
                source_id: "bom_obs_json".into(),
                display_name: "BOM Obs".into(),
                kind: SourceKind::Observation,
                fetched_at: now,
                weight: 1.5,
                current: WeatherPoint {
                    valid_at: now,
                    temp_c: Some(24.0),
                    feels_like_c: Some(23.0),
                    feels_like_is_derived: false,
                    rain_prob_pct: None,
                    rain_mm: Some(0.0),
                    wind_kmh: Some(15.0),
                    humidity_pct: Some(60.0),
                    solar_wm2: None,
                    summary: None,
                },
                hourly: vec![],
                source_url: None,
            },
            SourceReport {
                source_id: "open_meteo_bom_access".into(),
                display_name: "Open-Meteo BOM".into(),
                kind: SourceKind::Forecast,
                fetched_at: now,
                weight: 1.2,
                current: WeatherPoint {
                    valid_at: now,
                    temp_c: Some(25.0),
                    feels_like_c: Some(24.2),
                    feels_like_is_derived: false,
                    rain_prob_pct: Some(70.0),
                    rain_mm: Some(1.4),
                    wind_kmh: Some(18.0),
                    humidity_pct: Some(65.0),
                    solar_wm2: None,
                    summary: None,
                },
                hourly: vec![],
                source_url: None,
            },
            SourceReport {
                source_id: "met_no".into(),
                display_name: "MET Norway".into(),
                kind: SourceKind::Forecast,
                fetched_at: now,
                weight: 1.05,
                current: WeatherPoint {
                    valid_at: now,
                    temp_c: Some(23.5),
                    feels_like_c: Some(22.5),
                    feels_like_is_derived: true,
                    rain_prob_pct: Some(55.0),
                    rain_mm: Some(0.8),
                    wind_kmh: Some(14.0),
                    humidity_pct: Some(62.0),
                    solar_wm2: None,
                    summary: None,
                },
                hourly: vec![],
                source_url: None,
            },
        ];

        let bundle =
            aggregate_node_bundle(&test_node(), &reports, now, &AggregationConfig::default())
                .unwrap();
        assert_eq!(bundle.current_best_estimate.source_count, 3);
        assert_eq!(
            bundle.current_best_estimate.confidence,
            ConfidenceLevel::High
        );
        assert_eq!(
            bundle.current_best_estimate.rain.verdict,
            RainVerdict::Likely
        );
        assert!(bundle.current_best_estimate.temp_c > 23.5);
        assert!(bundle.current_best_estimate.temp_c < 25.0);
    }

    #[test]
    fn detects_active_rain_from_observation() {
        let now = Utc::now();
        let reports = vec![SourceReport {
            source_id: "bom_obs_json".into(),
            display_name: "BOM Obs".into(),
            kind: SourceKind::Observation,
            fetched_at: now,
            weight: 1.5,
            current: WeatherPoint {
                valid_at: now,
                temp_c: Some(19.0),
                feels_like_c: Some(17.5),
                feels_like_is_derived: false,
                rain_prob_pct: None,
                rain_mm: Some(1.2),
                wind_kmh: Some(20.0),
                humidity_pct: Some(88.0),
                solar_wm2: None,
                summary: None,
            },
            hourly: vec![],
            source_url: None,
        }];
        let bundle =
            aggregate_node_bundle(&test_node(), &reports, now, &AggregationConfig::default())
                .unwrap();
        assert_eq!(
            bundle.current_best_estimate.rain.verdict,
            RainVerdict::Raining
        );
        assert!(bundle.current_best_estimate.rain.is_raining_now);
    }
}
