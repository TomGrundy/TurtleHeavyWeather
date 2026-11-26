use chrono::Duration;
use leptos::prelude::*;
use thw_core::HourlyEstimate;

use crate::components::{ChartPoint, InteractiveForecastChart};
use crate::time::{format_local_short, now_utc};

#[component]
pub fn SevenDayOutlook(hourly: Vec<HourlyEstimate>) -> impl IntoView {
    let cutoff = now_utc() - Duration::minutes(30);
    let upcoming: Vec<HourlyEstimate> = {
        let filtered: Vec<_> = hourly
            .iter()
            .filter(|h| h.valid_at >= cutoff)
            .cloned()
            .collect();
        if filtered.len() >= 24 {
            filtered
        } else {
            hourly
        }
    };

    // Skip the first 24h (shown in the primary BestEstimateCard), then group
    // the following 6 days into 24-hour collapsible periods.
    let later: Vec<HourlyEstimate> = upcoming.into_iter().skip(24).collect();
    if later.is_empty() {
        return view! { <div /> }.into_any();
    }

    let day_chunks: Vec<(usize, Vec<HourlyEstimate>)> = later
        .chunks(24)
        .take(6)
        .enumerate()
        .map(|(idx, chunk)| (idx + 2, chunk.to_vec()))
        .collect();

    view! {
        <section class="sources-section">
            <h2>"Next 6 Days (24-Hour Outlook)"</h2>
            {day_chunks.into_iter().map(|(day_num, chunk)| {
                let first_lbl = chunk.first().map(|h| format_local_short(h.valid_at)).unwrap_or_default();
                let last_lbl = chunk.last().map(|h| format_local_short(h.valid_at)).unwrap_or_default();
                let header = format!("Day {day_num}: {first_lbl} – {last_lbl}");

                let fl_min = chunk.iter().map(|h| h.feels_like_c).fold(f64::INFINITY, f64::min);
                let fl_max = chunk.iter().map(|h| h.feels_like_c).fold(f64::NEG_INFINITY, f64::max);
                let t_min = chunk.iter().map(|h| h.temp_c).fold(f64::INFINITY, f64::min);
                let t_max = chunk.iter().map(|h| h.temp_c).fold(f64::NEG_INFINITY, f64::max);
                let max_rain_pct = chunk.iter().map(|h| h.rain_prob_pct).fold(0.0_f64, f64::max);
                let total_rain_mm: f64 = chunk.iter().map(|h| h.rain_mm_expected).sum();

                let chart_points: Vec<ChartPoint> = chunk
                    .into_iter()
                    .map(|h| ChartPoint {
                        valid_at: h.valid_at,
                        temp_c: Some(h.temp_c),
                        feels_like_c: Some(h.feels_like_c),
                        feels_like_sun_c: h.feels_like_sun_c,
                        rain_prob_pct: Some(h.rain_prob_pct),
                        rain_mm: Some(h.rain_mm_expected),
                    })
                    .collect();

                view! {
                    <details class="source-accordion">
                        <summary>
                            <span>{header}</span>
                            <span class="source-summary-stats">
                                <span>{format!("Feels {:.0}–{:.0}°C", fl_min, fl_max)}</span>
                                <span>{format!("Temp {:.0}–{:.0}°C", t_min, t_max)}</span>
                                <span>{format!("Rain up to {:.0}% ({:.1} mm)", max_rain_pct, total_rain_mm)}</span>
                            </span>
                        </summary>
                        <div class="source-body">
                            <InteractiveForecastChart
                                points=chart_points
                                title="24-Hour Period Forecast (Hover or Tap)"
                            />
                        </div>
                    </details>
                }
            }).collect_view()}
        </section>
    }
    .into_any()
}
