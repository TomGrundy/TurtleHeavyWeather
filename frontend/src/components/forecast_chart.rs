use chrono::{DateTime, Utc};
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::time::format_local_hhmm;

#[derive(Debug, Clone, PartialEq)]
pub struct ChartPoint {
    pub valid_at: DateTime<Utc>,
    pub temp_c: Option<f64>,
    pub feels_like_c: Option<f64>,
    pub feels_like_sun_c: Option<f64>,
    pub rain_prob_pct: Option<f64>,
    pub rain_mm: Option<f64>,
}

/// Builds a smooth cubic Bezier SVG path through `(x, y)` points using
/// Catmull-Rom to cubic Bezier control-point conversion, optionally clamping
/// control-point Y values within `[min_y, max_y]` so sharp 0%→100% transitions
/// never overshoot past the chart bounds.
pub fn build_smooth_curve_path_clamped(pts: &[(f64, f64)], y_bounds: Option<(f64, f64)>) -> String {
    if pts.is_empty() {
        return String::new();
    }
    if pts.len() == 1 {
        return format!("M {:.1} {:.1}", pts[0].0, pts[0].1);
    }

    let clamp_y = |y: f64| -> f64 {
        match y_bounds {
            Some((lo, hi)) => y.clamp(lo, hi),
            None => y,
        }
    };

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
        let c1y = clamp_y(p1.1 + (p2.1 - p0.1) / 6.0);
        let c2x = p2.0 - (p3.0 - p1.0) / 6.0;
        let c2y = clamp_y(p2.1 - (p3.1 - p1.1) / 6.0);

        d.push_str(&format!(
            " C {:.1} {:.1}, {:.1} {:.1}, {:.1} {:.1}",
            c1x, c1y, c2x, c2y, p2.0, p2.1
        ));
    }
    d
}

pub fn build_smooth_curve_path(pts: &[(f64, f64)]) -> String {
    build_smooth_curve_path_clamped(pts, None)
}

/// Builds a closed SVG area path from the smooth curve down to `baseline_y`.
pub fn build_area_path(pts: &[(f64, f64)], baseline_y: f64) -> String {
    build_area_path_clamped(pts, baseline_y, None)
}

pub fn build_area_path_clamped(
    pts: &[(f64, f64)],
    baseline_y: f64,
    y_bounds: Option<(f64, f64)>,
) -> String {
    if pts.is_empty() {
        return String::new();
    }
    let mut d = build_smooth_curve_path_clamped(pts, y_bounds);
    let last_x = pts[pts.len() - 1].0;
    let first_x = pts[0].0;
    d.push_str(&format!(
        " L {:.1} {:.1} L {:.1} {:.1} Z",
        last_x, baseline_y, first_x, baseline_y
    ));
    d
}

/// Maps a client X coordinate inside an SVG bounding rect to the nearest hourly index.
pub fn nearest_index_from_client_x(
    client_x: f64,
    rect_left: f64,
    rect_width: f64,
    svg_width: f64,
    pad_l: f64,
    plot_w: f64,
    len: usize,
) -> Option<usize> {
    if len < 2 || rect_width <= 0.0 || plot_w <= 0.0 {
        return None;
    }
    let svg_x = ((client_x - rect_left) / rect_width) * svg_width;
    let frac = ((svg_x - pad_l) / plot_w).clamp(0.0, 1.0);
    let idx = (frac * (len - 1) as f64).round() as usize;
    Some(idx.min(len - 1))
}

#[component]
pub fn InteractiveForecastChart(
    points: Vec<ChartPoint>,
    #[prop(default = "Hourly Forecast (Hover or Tap)")] title: &'static str,
) -> impl IntoView {
    let has_feels = points.iter().filter(|p| p.feels_like_c.is_some()).count() >= 2;
    let has_temp = points.iter().filter(|p| p.temp_c.is_some()).count() >= 2;
    let has_rain = points.iter().filter(|p| p.rain_prob_pct.is_some()).count() >= 2;

    if points.len() < 2 || (!has_feels && !has_temp && !has_rain) {
        return view! { <div /> }.into_any();
    }

    let width = 760.0_f64;
    let height = 235.0_f64;
    let pad_l = 32.0_f64;
    let pad_r = if has_rain { 34.0_f64 } else { 14.0_f64 };
    let pad_t = 18.0_f64;
    let pad_b = 32.0_f64;
    let plot_w = width - pad_l - pad_r;
    let plot_h = height - pad_t - pad_b;
    let bottom_y = pad_t + plot_h;

    let temp_vals: Vec<f64> = points
        .iter()
        .flat_map(|h| [h.feels_like_c, h.temp_c].into_iter().flatten())
        .collect();
    let (t_min, t_max) = if temp_vals.is_empty() {
        (0.0, 30.0)
    } else {
        let min_t_raw = temp_vals.iter().copied().fold(f64::INFINITY, f64::min);
        let max_t_raw = temp_vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let lo = (min_t_raw - 2.0).floor();
        let hi = (max_t_raw + 2.0).ceil().max(lo + 4.0);
        (lo, hi)
    };
    let t_span = t_max - t_min;

    let n = points.len();
    let x_for = move |idx: usize| -> f64 { pad_l + (idx as f64 / (n - 1) as f64) * plot_w };
    let y_temp = move |t: f64| -> f64 { bottom_y - ((t - t_min) / t_span) * plot_h };
    let y_rain = move |pct: f64| -> f64 { bottom_y - (pct.clamp(0.0, 100.0) / 100.0) * plot_h };

    let rain_pts: Vec<(f64, f64)> = if has_rain {
        points
            .iter()
            .enumerate()
            .filter_map(|(i, h)| h.rain_prob_pct.map(|p| (x_for(i), y_rain(p))))
            .collect()
    } else {
        Vec::new()
    };
    let temp_pts: Vec<(f64, f64)> = if has_temp {
        points
            .iter()
            .enumerate()
            .filter_map(|(i, h)| h.temp_c.map(|t| (x_for(i), y_temp(t))))
            .collect()
    } else {
        Vec::new()
    };
    let feels_pts: Vec<(f64, f64)> = if has_feels {
        points
            .iter()
            .enumerate()
            .filter_map(|(i, h)| h.feels_like_c.map(|f| (x_for(i), y_temp(f))))
            .collect()
    } else {
        Vec::new()
    };

    let y_bounds = Some((pad_t, bottom_y));
    let rain_area_d = build_area_path_clamped(&rain_pts, bottom_y, y_bounds);
    let rain_line_d = build_smooth_curve_path_clamped(&rain_pts, y_bounds);
    let temp_line_d = build_smooth_curve_path_clamped(&temp_pts, y_bounds);
    let feels_line_d = build_smooth_curve_path_clamped(&feels_pts, y_bounds);

    let y_mid_t = (t_min + t_max) / 2.0;
    let temp_ticks = if has_temp || has_feels {
        vec![
            (t_max, pad_t + 4.0),
            (y_mid_t, pad_t + plot_h / 2.0 + 4.0),
            (t_min, bottom_y + 3.0),
        ]
    } else {
        Vec::new()
    };

    let x_step = (n / 5).max(1);
    let x_labels: Vec<(f64, String)> = points
        .iter()
        .enumerate()
        .filter(|(i, _)| i % x_step == 0 || *i == n - 1)
        .map(|(i, h)| (x_for(i), format_local_hhmm(h.valid_at)))
        .collect();

    let (selected_idx, set_selected_idx) = signal(Option::<usize>::None);
    let points_stored = StoredValue::new(points);

    let update_from_client_x = move |target: Option<web_sys::EventTarget>, client_x: f64| {
        let Some(t) = target else { return };
        let Ok(el) = t.dyn_into::<web_sys::Element>() else {
            return;
        };
        let rect = el.get_bounding_client_rect();
        if let Some(idx) = nearest_index_from_client_x(
            client_x,
            rect.left(),
            rect.width(),
            width,
            pad_l,
            plot_w,
            n,
        ) {
            set_selected_idx.set(Some(idx));
        }
    };

    view! {
        <div class="chart-card">
            <div class="chart-header">
                <span class="chart-title">{title}</span>
                <div class="chart-legend">
                    {has_feels.then(|| view! {
                        <span class="legend-item"><span class="legend-swatch swatch-feels" />"Feels Like (°C)"</span>
                    })}
                    {has_temp.then(|| view! {
                        <span class="legend-item"><span class="legend-swatch swatch-temp" />"Temp (°C)"</span>
                    })}
                    {has_rain.then(|| view! {
                        <span class="legend-item"><span class="legend-swatch swatch-rain" />"Rain Chance (%)"</span>
                    })}
                </div>
            </div>

            {move || match selected_idx.get().and_then(|idx| {
                points_stored.with_value(|list| list.get(idx).cloned())
            }) {
                Some(pt) => {
                    let sun_note = match (pt.feels_like_c, pt.feels_like_sun_c) {
                        (Some(fl), Some(sun)) if sun > fl + 0.3 => format!(" (Sun ~{sun:.1}°C)"),
                        _ => String::new(),
                    };
                    view! {
                        <div class="chart-tooltip-bar chart-tooltip-active">
                            <strong>{format_local_hhmm(pt.valid_at)}</strong>
                            {pt.feels_like_c.map(|fl| view! {
                                <span>{format!("Feels Like: {fl:.1}°C{sun_note}")}</span>
                            })}
                            {pt.temp_c.map(|t| view! {
                                <span>{format!("Actual Temp: {t:.1}°C")}</span>
                            })}
                            {pt.rain_prob_pct.map(|rp| {
                                let mm_part = pt.rain_mm.map(|mm| format!(" ({mm:.1} mm)")).unwrap_or_default();
                                view! {
                                    <span>{format!("Rain: {rp:.0}%{mm_part}")}</span>
                                }
                            })}
                        </div>
                    }.into_any()
                }
                None => view! {
                    <div class="chart-tooltip-bar chart-tooltip-placeholder">
                        <span>"Hover or tap the chart to inspect hourly values"</span>
                    </div>
                }.into_any(),
            }}

            <svg
                class="forecast-svg"
                viewBox=format!("0 0 {width} {height}")
                role="img"
                aria-label="Hourly weather chart"
                on:mousemove=move |ev: web_sys::MouseEvent| {
                    update_from_client_x(ev.current_target(), ev.client_x() as f64);
                }
                on:click=move |ev: web_sys::MouseEvent| {
                    update_from_client_x(ev.current_target(), ev.client_x() as f64);
                }
                on:mouseleave=move |_| set_selected_idx.set(None)
                on:touchstart=move |ev: web_sys::TouchEvent| {
                    if let Some(touch) = ev.touches().get(0) {
                        update_from_client_x(ev.current_target(), touch.client_x() as f64);
                    }
                }
                on:touchmove=move |ev: web_sys::TouchEvent| {
                    if let Some(touch) = ev.touches().get(0) {
                        update_from_client_x(ev.current_target(), touch.client_x() as f64);
                    }
                }
            >
                <line x1=pad_l y1=pad_t x2={width - pad_r} y2=pad_t class="chart-grid" />
                <line x1=pad_l y1={pad_t + plot_h / 2.0} x2={width - pad_r} y2={pad_t + plot_h / 2.0} class="chart-grid" />
                <line x1=pad_l y1=bottom_y x2={width - pad_r} y2=bottom_y class="chart-axis" />

                {has_rain.then(|| view! {
                    <g>
                        <path d=rain_area_d fill="rgba(96, 165, 250, 0.28)" />
                        <path d=rain_line_d fill="none" stroke="rgba(96, 165, 250, 0.7)" stroke-width="1.5" />
                    </g>
                })}

                {has_temp.then(|| view! {
                    <path d=temp_line_d fill="none" stroke="#fbbf24" stroke-width="2.5" stroke-linecap="round" />
                })}

                {has_feels.then(|| view! {
                    <path d=feels_line_d fill="none" stroke="#22d3ee" stroke-width="3" stroke-linecap="round" />
                })}

                {move || selected_idx.get().and_then(|idx| {
                    points_stored.with_value(|list| list.get(idx).cloned().map(|pt| (idx, pt)))
                }).map(|(idx, pt)| {
                    let cx = x_for(idx);
                    view! {
                        <g class="chart-crosshair-group">
                            <line
                                x1=cx y1=pad_t x2=cx y2=bottom_y
                                stroke="rgba(238, 243, 251, 0.75)"
                                stroke-width="1.5"
                                stroke-dasharray="4 3"
                            />
                            {pt.rain_prob_pct.filter(|_| has_rain).map(|rp| view! {
                                <circle cx=cx cy={y_rain(rp)} r="4.5" fill="#60a5fa" stroke="#0b1220" stroke-width="1.5" />
                            })}
                            {pt.temp_c.filter(|_| has_temp).map(|t| view! {
                                <circle cx=cx cy={y_temp(t)} r="5" fill="#fbbf24" stroke="#0b1220" stroke-width="1.5" />
                            })}
                            {pt.feels_like_c.filter(|_| has_feels).map(|fl| view! {
                                <circle cx=cx cy={y_temp(fl)} r="5.5" fill="#22d3ee" stroke="#0b1220" stroke-width="1.5" />
                            })}
                        </g>
                    }
                })}

                {temp_ticks.into_iter().map(|(val, y)| view! {
                    <text x={pad_l - 6.0} y=y text-anchor="end" class="chart-tick">{format!("{val:.0}°")}</text>
                }).collect_view()}

                {has_rain.then(|| view! {
                    <g>
                        <text x={width - pad_r + 6.0} y={pad_t + 4.0} text-anchor="start" class="chart-tick chart-tick-rain">"100%"</text>
                        <text x={width - pad_r + 6.0} y={pad_t + plot_h / 2.0 + 4.0} text-anchor="start" class="chart-tick chart-tick-rain">"50%"</text>
                        <text x={width - pad_r + 6.0} y={bottom_y + 3.0} text-anchor="start" class="chart-tick chart-tick-rain">"0%"</text>
                    </g>
                })}

                {x_labels.into_iter().map(|(x, lbl)| view! {
                    <text x=x y={height - 8.0} text-anchor="middle" class="chart-tick">{lbl}</text>
                }).collect_view()}
            </svg>
        </div>
    }
    .into_any()
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

    #[test]
    fn maps_client_x_to_nearest_hourly_index() {
        let idx = nearest_index_from_client_x(380.0, 0.0, 760.0, 760.0, 42.0, 676.0, 24);
        assert_eq!(idx, Some(12));
    }
}
