use std::collections::BTreeMap;

use leptos::prelude::*;
use thw_core::HourlyEstimate;

use crate::components::{ChartPoint, InteractiveForecastChart};
use crate::time::{local_date_key, local_weekday_name, now_utc};

#[component]
pub fn SevenDayOutlook(hourly: Vec<HourlyEstimate>) -> impl IntoView {
    let today_key = local_date_key(now_utc());

    // Group hourly estimates into midnight-to-midnight local calendar days,
    // skipping today (already covered by the top 24-hour hero section).
    let mut by_day: BTreeMap<(i32, u32, u32), Vec<HourlyEstimate>> = BTreeMap::new();
    for pt in hourly {
        let key = local_date_key(pt.valid_at);
        if key > today_key {
            by_day.entry(key).or_default().push(pt);
        }
    }

    let day_chunks: Vec<(String, Vec<HourlyEstimate>)> = by_day
        .into_values()
        .take(6)
        .filter_map(|chunk| {
            let label = chunk.first().map(|h| local_weekday_name(h.valid_at))?;
            Some((label, chunk))
        })
        .collect();

    if day_chunks.is_empty() {
        return view! { <div /> }.into_any();
    }

    view! {
        <section class="sources-section">
            <h2>"Next 6 Days"</h2>
            {day_chunks.into_iter().map(|(weekday, chunk)| {
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
                            <span>{weekday}</span>
                            <span class="source-summary-stats">
                                <span>{format!("Feels {:.0}–{:.0}°C", fl_min, fl_max)}</span>
                                <span>{format!("Temp {:.0}–{:.0}°C", t_min, t_max)}</span>
                                <span>{format!("Rain up to {:.0}% ({:.1} mm)", max_rain_pct, total_rain_mm)}</span>
                            </span>
                        </summary>
                        <div class="source-body">
                            <InteractiveForecastChart
                                points=chart_points
                                title="Midnight–Midnight Forecast (Hover or Tap)"
                            />
                        </div>
                    </details>
                }
            }).collect_view()}
        </section>
    }
    .into_any()
}
