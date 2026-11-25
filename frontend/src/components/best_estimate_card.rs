use leptos::prelude::*;
use thw_core::{ConfidenceLevel, LocationIndexEntry, NodeWeatherBundle, RainVerdict};

use crate::components::{ChartPoint, InteractiveForecastChart};
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

    let sun_sub = est
        .feels_like_sun_c
        .filter(|s| *s > est.feels_like_c + 0.3)
        .map(|s| format!("In direct sun: ~{s:.1}°C"))
        .unwrap_or_else(|| {
            format!(
                "Spread: {:.1}° .. {:.1}°",
                est.spread.feels_like_min_c, est.spread.feels_like_max_c
            )
        });

    let hourly_chart: Vec<ChartPoint> = bundle
        .hourly_best_estimate
        .iter()
        .take(24)
        .map(|h| ChartPoint {
            valid_at: h.valid_at,
            temp_c: Some(h.temp_c),
            feels_like_c: Some(h.feels_like_c),
            feels_like_sun_c: h.feels_like_sun_c,
            rain_prob_pct: Some(h.rain_prob_pct),
            rain_mm: Some(h.rain_mm_expected),
        })
        .collect();
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
                    <div class="metric-sub">{sun_sub}</div>
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

            <InteractiveForecastChart points=hourly_chart />

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
