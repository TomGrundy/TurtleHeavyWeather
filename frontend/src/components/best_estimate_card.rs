use leptos::prelude::*;
use thw_core::{
    ConfidenceLevel, HourlyEstimate, LocationIndexEntry, NodeWeatherBundle, RainVerdict,
};

use crate::time::{format_local_hhmm, format_local_short};

fn rain_badge_class(verdict: RainVerdict) -> &'static str {
    match verdict {
        RainVerdict::Dry => "rain-badge rain-dry",
        RainVerdict::Possible => "rain-badge rain-possible",
        RainVerdict::Likely => "rain-badge rain-likely",
        RainVerdict::Raining => "rain-badge rain-raining",
    }
}

fn confidence_label(conf: ConfidenceLevel) -> &'static str {
    match conf {
        ConfidenceLevel::High => "High agreement",
        ConfidenceLevel::Medium => "Moderate agreement",
        ConfidenceLevel::Low => "Low agreement",
    }
}

/// Builds a smooth cubic Bezier SVG path through `(x, y)` points using
/// Catmull-Rom to cubic Bezier control-point conversion.
pub fn build_smooth_curve_path(pts: &[(f64, f64)]) -> String {
    if pts.is_empty() {
        return String::new();
    }
    if pts.len() == 1 {
        return format!("M {:.1} {:.1}", pts[0].0, pts[0].1);
    }

    let mut d = format!("M {:.1} {:.1}", pts[0].0, pts[0].1);
    for i in 0..pts.len() - 1 {
        let p0 = if i == 0 { pts[0] } else { pts[i - 1] };
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = if i + 2 < pts.len() {
            pts[i + 2]
        } else {
            pts[pts.len() - 1]
        };

        let c1x = p1.0 + (p2.0 - p0.0) / 6.0;
        let c1y = p1.1 + (p2.1 - p0.1) / 6.0;
        let c2x = p2.0 - (p3.0 - p1.0) / 6.0;
        let c2y = p2.1 - (p3.1 - p1.1) / 6.0;

        d.push_str(&format!(
            " C {:.1} {:.1}, {:.1} {:.1}, {:.1} {:.1}",
            c1x, c1y, c2x, c2y, p2.0, p2.1
        ));
    }
    d
}

/// Builds a closed SVG area path from the smooth curve down to `baseline_y`.
pub fn build_area_path(pts: &[(f64, f64)], baseline_y: f64) -> String {
    if pts.is_empty() {
        return String::new();
    }
    let mut d = build_smooth_curve_path(pts);
    let last_x = pts[pts.len() - 1].0;
    let first_x = pts[0].0;
    d.push_str(&format!(
        " L {:.1} {:.1} L {:.1} {:.1} Z",
        last_x, baseline_y, first_x, baseline_y
    ));
    d
}

#[component]
fn HourlyForecastChart(hourly: Vec<HourlyEstimate>) -> impl IntoView {
    if hourly.len() < 2 {
        return view! { <div /> }.into_any();
    }

    let width = 760.0_f64;
    let height = 220.0_f64;
    let pad_l = 42.0_f64;
    let pad_r = 42.0_f64;
    let pad_t = 18.0_f64;
    let pad_b = 30.0_f64;
    let plot_w = width - pad_l - pad_r;
    let plot_h = height - pad_t - pad_b;
    let bottom_y = pad_t + plot_h;

    let min_t_raw = hourly
        .iter()
        .flat_map(|h| [h.feels_like_c, h.temp_c])
        .fold(f64::INFINITY, f64::min);
    let max_t_raw = hourly
        .iter()
        .flat_map(|h| [h.feels_like_c, h.temp_c])
        .fold(f64::NEG_INFINITY, f64::max);
    let t_min = (min_t_raw - 2.0).floor();
    let t_max = (max_t_raw + 2.0).ceil().max(t_min + 4.0);
    let t_span = t_max - t_min;

    let n = hourly.len();
    let x_for = |idx: usize| -> f64 { pad_l + (idx as f64 / (n - 1) as f64) * plot_w };
    let y_temp = |t: f64| -> f64 { bottom_y - ((t - t_min) / t_span) * plot_h };
    let y_rain = |pct: f64| -> f64 { bottom_y - (pct.clamp(0.0, 100.0) / 100.0) * plot_h };

    let rain_pts: Vec<(f64, f64)> = hourly
        .iter()
        .enumerate()
        .map(|(i, h)| (x_for(i), y_rain(h.rain_prob_pct)))
        .collect();
    let temp_pts: Vec<(f64, f64)> = hourly
        .iter()
        .enumerate()
        .map(|(i, h)| (x_for(i), y_temp(h.temp_c)))
        .collect();
    let feels_pts: Vec<(f64, f64)> = hourly
        .iter()
        .enumerate()
        .map(|(i, h)| (x_for(i), y_temp(h.feels_like_c)))
        .collect();

    let rain_area_d = build_area_path(&rain_pts, bottom_y);
    let rain_line_d = build_smooth_curve_path(&rain_pts);
    let temp_line_d = build_smooth_curve_path(&temp_pts);
    let feels_line_d = build_smooth_curve_path(&feels_pts);

    let y_mid_t = (t_min + t_max) / 2.0;
    let temp_ticks = [
        (t_max, pad_t + 4.0),
        (y_mid_t, pad_t + plot_h / 2.0 + 4.0),
        (t_min, bottom_y + 3.0),
    ];

    let x_step = (n / 6).max(1);
    let x_labels: Vec<(f64, String)> = hourly
        .iter()
        .enumerate()
        .filter(|(i, _)| i % x_step == 0 || *i == n - 1)
        .map(|(i, h)| (x_for(i), format_local_hhmm(h.valid_at)))
        .collect();

    view! {
        <div class="chart-card">
            <div class="chart-header">
                <span class="chart-title">"Hourly Forecast (Local Time)"</span>
                <div class="chart-legend">
                    <span class="legend-item"><span class="legend-swatch swatch-feels" />"Feels Like (°C)"</span>
                    <span class="legend-item"><span class="legend-swatch swatch-temp" />"Temp (°C)"</span>
                    <span class="legend-item"><span class="legend-swatch swatch-rain" />"Rain Chance (%)"</span>
                </div>
            </div>

            <svg
                class="forecast-svg"
                viewBox=format!("0 0 {width} {height}")
                role="img"
                aria-label="Hourly feels like temperature, actual temperature, and rain probability chart"
            >
                <defs>
                    <linearGradient id="rainAreaGrad" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="0%" stop-color="rgba(96, 165, 250, 0.42)" />
                        <stop offset="100%" stop-color="rgba(96, 165, 250, 0.06)" />
                    </linearGradient>
                </defs>

                // Horizontal gridlines
                <line x1=pad_l y1=pad_t x2={width - pad_r} y2=pad_t class="chart-grid" />
                <line x1=pad_l y1={pad_t + plot_h / 2.0} x2={width - pad_r} y2={pad_t + plot_h / 2.0} class="chart-grid" />
                <line x1=pad_l y1=bottom_y x2={width - pad_r} y2=bottom_y class="chart-axis" />

                // Rain % filled area behind the temperature curves
                <path d=rain_area_d fill="url(#rainAreaGrad)" />
                <path d=rain_line_d fill="none" stroke="rgba(96, 165, 250, 0.7)" stroke-width="1.5" />

                // Actual temperature curve
                <path d=temp_line_d fill="none" stroke="#fbbf24" stroke-width="2.5" stroke-linecap="round" />

                // Feels-like temperature curve (foreground)
                <path d=feels_line_d fill="none" stroke="#22d3ee" stroke-width="3" stroke-linecap="round" />

                // Data point dots with native hover tooltips
                {hourly.iter().enumerate().map(|(i, h)| {
                    let x = x_for(i);
                    let yt = y_temp(h.temp_c);
                    let yf = y_temp(h.feels_like_c);
                    let tip = format!(
                        "{}: Feels {:.1}°C, Temp {:.1}°C, Rain {:.0}%",
                        format_local_hhmm(h.valid_at),
                        h.feels_like_c,
                        h.temp_c,
                        h.rain_prob_pct
                    );
                    view! {
                        <g>
                            <circle cx=x cy=yt r="3" fill="#fbbf24"><title>{tip.clone()}</title></circle>
                            <circle cx=x cy=yf r="3.5" fill="#22d3ee"><title>{tip}</title></circle>
                        </g>
                    }
                }).collect_view()}

                // Left Y-axis temperature labels
                {temp_ticks.into_iter().map(|(val, y)| view! {
                    <text x={pad_l - 6.0} y=y text-anchor="end" class="chart-tick">{format!("{val:.0}°")}</text>
                }).collect_view()}

                // Right Y-axis rain % labels
                <text x={width - pad_r + 6.0} y={pad_t + 4.0} text-anchor="start" class="chart-tick chart-tick-rain">"100%"</text>
                <text x={width - pad_r + 6.0} y={pad_t + plot_h / 2.0 + 4.0} text-anchor="start" class="chart-tick chart-tick-rain">"50%"</text>
                <text x={width - pad_r + 6.0} y={bottom_y + 3.0} text-anchor="start" class="chart-tick chart-tick-rain">"0%"</text>

                // X-axis local time labels
                {x_labels.into_iter().map(|(x, lbl)| view! {
                    <text x=x y={height - 8.0} text-anchor="middle" class="chart-tick">{lbl}</text>
                }).collect_view()}
            </svg>
        </div>
    }
    .into_any()
}

#[component]
pub fn BestEstimateCard(
    bundle: NodeWeatherBundle,
    selected_location: Option<LocationIndexEntry>,
) -> impl IntoView {
    let est = bundle.current_best_estimate.clone();
    let title = selected_location
        .as_ref()
        .map(|l| format!("{}, {:?} {}", l.suburb, l.state, l.postcode))
        .unwrap_or_else(|| bundle.node.name.clone());
    let station_meta = format!(
        "Nearest node: {} • {} sources • {} • Updated {}",
        bundle.node.name,
        est.source_count,
        confidence_label(est.confidence),
        format_local_short(bundle.generated_at)
    );

    let hourly_chart = bundle
        .hourly_best_estimate
        .iter()
        .take(24)
        .cloned()
        .collect::<Vec<_>>();
    let hourly_strip = bundle
        .hourly_best_estimate
        .into_iter()
        .take(12)
        .collect::<Vec<_>>();

    view! {
        <section class="hero-card">
            <div class="hero-header">
                <div>
                    <div class="hero-location">{title}</div>
                    <div class="hero-meta">{station_meta}</div>
                </div>
            </div>

            <div class="hero-metrics">
                <div class="metric-box">
                    <div class="metric-label">"Feels Like"</div>
                    <div class="metric-value">{format!("{:.1}°C", est.feels_like_c)}</div>
                    <div class="metric-sub">
                        {format!("Spread: {:.1}° .. {:.1}°", est.spread.feels_like_min_c, est.spread.feels_like_max_c)}
                    </div>
                </div>

                <div class="metric-box">
                    <div class="metric-label">"Temperature"</div>
                    <div class="metric-value">{format!("{:.1}°C", est.temp_c)}</div>
                    <div class="metric-sub">
                        {format!("Spread: {:.1}° .. {:.1}°", est.spread.temp_min_c, est.spread.temp_max_c)}
                    </div>
                </div>

                <div class="metric-box">
                    <div class="metric-label">"Will It Rain?"</div>
                    <div class="metric-value">{format!("{:.0}%", est.rain.probability_pct)}</div>
                    <div>
                        <span class=rain_badge_class(est.rain.verdict)>{est.rain.label.clone()}</span>
                    </div>
                    <div class="metric-sub">
                        {format!("Expected: {:.1} mm", est.rain.expected_mm)}
                    </div>
                </div>
            </div>

            <HourlyForecastChart hourly=hourly_chart />

            {if hourly_strip.is_empty() {
                view! { <div /> }.into_any()
            } else {
                view! {
                    <div class="hourly-strip">
                        {hourly_strip.into_iter().map(|h| {
                            let time_lbl = format_local_hhmm(h.valid_at);
                            view! {
                                <div class="hourly-pill">
                                    <div class="hero-meta">{time_lbl}</div>
                                    <div><strong>{format!("{:.0}°", h.feels_like_c)}</strong></div>
                                    <div class="hero-meta">{format!("{:.0}° air", h.temp_c)}</div>
                                    <div class="hero-meta">{format!("{:.0}% rain", h.rain_prob_pct)}</div>
                                </div>
                            }
                        }).collect_view()}
                    </div>
                }.into_any()
            }}
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_smooth_curve_and_closed_rain_area() {
        let pts = [(0.0, 100.0), (50.0, 40.0), (100.0, 80.0)];
        let curve = build_smooth_curve_path(&pts);
        assert!(curve.starts_with("M 0.0 100.0 C "));
        let area = build_area_path(&pts, 180.0);
        assert!(area.ends_with("L 100.0 180.0 L 0.0 180.0 Z"));
    }
}
