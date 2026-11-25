use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::models::{ObservationRecord, RegressionMetricRow, RegressionSummary};

#[derive(Debug, Clone)]
pub struct RegressionForecastInput {
    pub node_id: String,
    pub subject_id: String,
    pub issued_at: DateTime<Utc>,
    pub valid_at: DateTime<Utc>,
    pub lead_hours: i64,
    pub temp_c: Option<f64>,
    pub feels_like_c: Option<f64>,
    pub rain_prob_pct: Option<f64>,
}

fn lead_bucket_for(lead_hours: i64) -> Option<&'static str> {
    if (0..=6).contains(&lead_hours) {
        Some("0-6h")
    } else if (7..=24).contains(&lead_hours) {
        Some("6-24h")
    } else if (25..=72).contains(&lead_hours) {
        Some("24-72h")
    } else {
        None
    }
}

fn is_sane_observation(obs: &ObservationRecord) -> bool {
    obs.temp_c
        .is_none_or(|t| (-15.0..=55.0).contains(&t) && t.is_finite())
        && obs
            .feels_like_c
            .is_none_or(|f| (-25.0..=65.0).contains(&f) && f.is_finite())
}

/// Selects the highest-quality observation within ±30 minutes of `valid_at`
/// (Tier 1 physical BOM AWS station observations win over Tier 2 fallbacks).
fn select_ground_truth<'a>(
    node_obs: &[&'a ObservationRecord],
    valid_at: DateTime<Utc>,
) -> Option<&'a ObservationRecord> {
    const MAX_DELTA_SECS: i64 = 1800;

    node_obs
        .iter()
        .copied()
        .filter(|obs| {
            is_sane_observation(obs)
                && (obs.observed_at - valid_at).num_seconds().abs() <= MAX_DELTA_SECS
        })
        .min_by_key(|obs| {
            (
                obs.quality_rank,
                (obs.observed_at - valid_at).num_seconds().abs(),
            )
        })
}

#[derive(Default)]
struct BucketAccum {
    sample_count: usize,
    temp_abs_errs: Vec<f64>,
    temp_sq_errs: Vec<f64>,
    temp_signed_errs: Vec<f64>,
    feels_abs_errs: Vec<f64>,
    feels_sq_errs: Vec<f64>,
    rain_briers: Vec<f64>,
    rain_hits: usize,
    rain_total: usize,
}

fn mean(vals: &[f64]) -> Option<f64> {
    if vals.is_empty() {
        None
    } else {
        Some((vals.iter().sum::<f64>() / vals.len() as f64 * 1000.0).round() / 1000.0)
    }
}

fn rmse(sq_vals: &[f64]) -> Option<f64> {
    if sq_vals.is_empty() {
        None
    } else {
        let m = sq_vals.iter().sum::<f64>() / sq_vals.len() as f64;
        Some((m.sqrt() * 1000.0).round() / 1000.0)
    }
}

pub fn evaluate_regression(
    forecasts: &[RegressionForecastInput],
    observations: &[ObservationRecord],
    computed_at: DateTime<Utc>,
    window_days: u32,
    display_names: &HashMap<String, String>,
) -> RegressionSummary {
    let mut obs_by_node: HashMap<&str, Vec<&ObservationRecord>> = HashMap::new();
    for obs in observations {
        obs_by_node
            .entry(obs.node_id.as_str())
            .or_default()
            .push(obs);
    }

    let mut groups: HashMap<(String, &'static str), BucketAccum> = HashMap::new();

    for fc in forecasts {
        let Some(bucket) = lead_bucket_for(fc.lead_hours) else {
            continue;
        };
        let Some(node_obs) = obs_by_node.get(fc.node_id.as_str()) else {
            continue;
        };
        let Some(truth) = select_ground_truth(node_obs, fc.valid_at) else {
            continue;
        };

        let acc = groups.entry((fc.subject_id.clone(), bucket)).or_default();
        acc.sample_count += 1;

        if let (Some(pred_t), Some(obs_t)) = (fc.temp_c, truth.temp_c) {
            let err = pred_t - obs_t;
            acc.temp_abs_errs.push(err.abs());
            acc.temp_sq_errs.push(err * err);
            acc.temp_signed_errs.push(err);
        }

        if let (Some(pred_fl), Some(obs_fl)) = (fc.feels_like_c, truth.feels_like_c) {
            let err = pred_fl - obs_fl;
            acc.feels_abs_errs.push(err.abs());
            acc.feels_sq_errs.push(err * err);
        }

        if let (Some(prob_pct), Some(rained)) = (fc.rain_prob_pct, truth.rained_bool) {
            let p = (prob_pct / 100.0).clamp(0.0, 1.0);
            let y = if rained { 1.0 } else { 0.0 };
            acc.rain_briers.push((p - y) * (p - y));
            acc.rain_total += 1;
            if (p >= 0.5) == rained {
                acc.rain_hits += 1;
            }
        }
    }

    let mut metrics: Vec<RegressionMetricRow> = groups
        .into_iter()
        .map(|((subject_id, lead_bucket), acc)| {
            let display_name = display_names
                .get(&subject_id)
                .cloned()
                .unwrap_or_else(|| subject_id.clone());
            RegressionMetricRow {
                computed_at,
                window_days,
                subject_id,
                display_name,
                lead_bucket: lead_bucket.to_string(),
                sample_count: acc.sample_count,
                temp_mae_c: mean(&acc.temp_abs_errs),
                temp_rmse_c: rmse(&acc.temp_sq_errs),
                temp_bias_c: mean(&acc.temp_signed_errs),
                feels_like_mae_c: mean(&acc.feels_abs_errs),
                feels_like_rmse_c: rmse(&acc.feels_sq_errs),
                rain_brier_score: mean(&acc.rain_briers),
                rain_hit_rate: if acc.rain_total > 0 {
                    Some((acc.rain_hits as f64 / acc.rain_total as f64 * 1000.0).round() / 1000.0)
                } else {
                    None
                },
            }
        })
        .collect();

    metrics.sort_by(|a, b| {
        a.subject_id
            .cmp(&b.subject_id)
            .then_with(|| a.lead_bucket.cmp(&b.lead_bucket))
    });

    // Compute suggested inverse-RMSE weights for individual sources with >= 5 samples in 0-6h or 6-24h.
    let mut inv_rmses: HashMap<String, f64> = HashMap::new();
    for row in &metrics {
        if row.subject_id != "__aggregate__"
            && (row.lead_bucket == "0-6h" || row.lead_bucket == "6-24h")
            && row.sample_count >= 5
        {
            if let Some(r) = row.temp_rmse_c {
                let score = 1.0 / (r + 0.25);
                inv_rmses
                    .entry(row.subject_id.clone())
                    .and_modify(|prev| *prev = (*prev + score) / 2.0)
                    .or_insert(score);
            }
        }
    }

    let total_score: f64 = inv_rmses.values().sum();
    let suggested_weights = if total_score > 0.0 {
        inv_rmses
            .into_iter()
            .map(|(k, v)| (k, (v / total_score * 1000.0).round() / 1000.0))
            .collect()
    } else {
        HashMap::new()
    };

    RegressionSummary {
        computed_at,
        window_days,
        metrics,
        suggested_weights,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn prefers_tier1_observation_and_computes_mae_rmse_bias_brier() {
        let base = Utc::now();
        let observations = vec![
            // Tier 2 fallback (wrong by +5°C)
            ObservationRecord {
                node_id: "bom-066062".into(),
                observed_at: base,
                source_id: "open_meteo_current".into(),
                quality_rank: 2,
                temp_c: Some(25.0),
                feels_like_c: Some(24.0),
                rain_mm_1h: Some(0.0),
                rained_bool: Some(false),
                wind_kmh: None,
                humidity_pct: None,
            },
            // Tier 1 BOM physical station (true temp = 20.0)
            ObservationRecord {
                node_id: "bom-066062".into(),
                observed_at: base,
                source_id: "bom_obs_json".into(),
                quality_rank: 1,
                temp_c: Some(20.0),
                feels_like_c: Some(19.0),
                rain_mm_1h: Some(1.0),
                rained_bool: Some(true),
                wind_kmh: None,
                humidity_pct: None,
            },
        ];

        // Constant +2°C warm bias, 80% rain probability on a rainy hour
        let forecasts = vec![RegressionForecastInput {
            node_id: "bom-066062".into(),
            subject_id: "met_no".into(),
            issued_at: base - Duration::hours(3),
            valid_at: base,
            lead_hours: 3,
            temp_c: Some(22.0),
            feels_like_c: Some(21.0),
            rain_prob_pct: Some(80.0),
        }];

        let summary = evaluate_regression(&forecasts, &observations, base, 7, &HashMap::new());
        assert_eq!(summary.metrics.len(), 1);
        let row = &summary.metrics[0];
        assert_eq!(row.lead_bucket, "0-6h");
        assert_eq!(row.temp_mae_c, Some(2.0));
        assert_eq!(row.temp_rmse_c, Some(2.0));
        assert_eq!(row.temp_bias_c, Some(2.0));
        // Brier = (0.8 - 1.0)^2 = 0.04
        assert_eq!(row.rain_brier_score, Some(0.04));
        assert_eq!(row.rain_hit_rate, Some(1.0));
    }
}
