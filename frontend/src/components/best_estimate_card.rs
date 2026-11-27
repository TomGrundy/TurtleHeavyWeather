use chrono::{DateTime, Duration, Utc};
use leptos::prelude::*;
use thw_core::{
    ConfidenceLevel, HourlyEstimate, LocationIndexEntry, NodeWeatherBundle, RainVerdict,
};

use crate::components::{ChartPoint, InteractiveForecastChart};
use crate::time::{format_local_hhmm, format_local_short, local_weekday_name, now_utc};

fn rain_badge_class(verdict: RainVerdict) -> &'static str {
    match verdict {
        RainVerdict::Dry => "rain-badge rain-dry",
        RainVerdict::Possible => "rain-badge rain-possible",
        RainVerdict::Likely => "rain-badge rain-likely",
        RainVerdict::Raining => "rain-badge rain-raining",
    }
}

fn uv_badge_class(index: f64) -> &'static str {
    if index >= 11.0 {
        "uv-badge uv-extreme"
    } else if index >= 8.0 {
        "uv-badge uv-very-high"
    } else if index >= 6.0 {
        "uv-badge uv-high"
    } else if index >= 3.0 {
        "uv-badge uv-moderate"
    } else {
        "uv-badge uv-low"
    }
}

fn format_spread(min_c: f64, max_c: f64) -> String {
    format!("Spread: {min_c:.1}° — {max_c:.1}°")
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

    let sun_boost = est.feels_like_sun_c.filter(|s| *s > est.feels_like_c + 0.3);

    let cutoff = now_utc() - Duration::minutes(30);
    let upcoming: Vec<_> = {
        let filtered: Vec<_> = bundle
            .hourly_best_estimate
            .iter()
            .filter(|h| h.valid_at >= cutoff)
            .cloned()
            .collect();
        if filtered.is_empty() {
            bundle.hourly_best_estimate.clone()
        } else {
            filtered
        }
    };

    let hourly_chart: Vec<ChartPoint> = upcoming
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
    let rain_outlook = summarize_rain_outlook(est.rain.is_raining_now, &upcoming);
    let hourly_strip = upcoming.into_iter().take(12).collect::<Vec<_>>();

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
                    {match sun_boost {
                        Some(s) => view! {
                            <div class="metric-sub sun-inline">
                                <span class="sun-badge">{format!("In direct sun: ~{s:.1}°C")}</span>
                            </div>
                        }.into_any(),
                        None => view! {
                            <div class="metric-sub">
                                {format_spread(est.spread.feels_like_min_c, est.spread.feels_like_max_c)}
                            </div>
                        }.into_any(),
                    }}
                    {est.uv.as_ref().map(|uv| {
                        let badge_cls = uv_badge_class(uv.index);
                        view! {
                            <div class="metric-sub uv-inline">
                                <span class=badge_cls>
                                    {format!("UV {:.1} · {}", uv.index, uv.category)}
                                </span>
                                <span class="uv-station">
                                    {format!(" ({})", uv.station_name)}
                                </span>
                            </div>
                        }
                    })}
                </div>

                <div class="metric-box">
                    <div class="metric-label">"Temperature"</div>
                    <div class="metric-value">{format!("{:.1}°C", est.temp_c)}</div>
                    <div class="metric-sub">
                        {format_spread(est.spread.temp_min_c, est.spread.temp_max_c)}
                    </div>
                </div>

                <div class="metric-box">
                    <div class="metric-label">"Will It Rain Today?"</div>
                    <div class="metric-value">{format!("{:.0}%", rain_outlook.today_max_prob)}</div>
                    <div>
                        <span class=rain_badge_class(rain_outlook.today_verdict)>{rain_outlook.today_label.clone()}</span>
                    </div>
                    <div class="metric-sub">
                        {if est.rain.is_raining_now {
                            format!("Raining now · {:.1} mm (24h)", rain_outlook.today_total_mm)
                        } else if let Some(start_at) = rain_outlook.today_start_at {
                            format!(
                                "Starts around {} · {:.1} mm (24h)",
                                format_local_hhmm(start_at),
                                rain_outlook.today_total_mm
                            )
                        } else {
                            format!("Expected (24h): {:.1} mm", rain_outlook.today_total_mm)
                        }}
                    </div>
                </div>
                {if !est.rain.is_raining_now && rain_outlook.today_start_at.is_none() {
                    let next_box = match &rain_outlook.next_rain_later {
                        Some(next) => {
                            let (verdict, label) =
                                classify_rain_level(next.rain_prob_pct, next.rain_mm_expected);
                            let when = format!(
                                "{} {}",
                                local_weekday_name(next.valid_at),
                                format_local_hhmm(next.valid_at)
                            );
                            view! {
                                <div class="metric-box">
                                    <div class="metric-label">"Next Rain"</div>
                                    <div class="metric-value">{when}</div>
                                    <div>
                                        <span class=rain_badge_class(verdict)>
                                            {format!("{label} ({:.0}%)", next.rain_prob_pct)}
                                        </span>
                                    </div>
                                    <div class="metric-sub">
                                        {format!("Expected: {:.1} mm", next.rain_mm_expected)}
                                    </div>
                                </div>
                            }
                            .into_any()
                        }
                        None => view! {
                            <div class="metric-box">
                                <div class="metric-label">"Next Rain"</div>
                                <div class="metric-value">"None"</div>
                                <div>
                                    <span class="badge badge-dry">"Dry 7 days"</span>
                                </div>
                                <div class="metric-sub">"No rain expected in the next 7 days"</div>
                            </div>
                        }
                        .into_any(),
                    };
                    Some(next_box)
                } else {
                    None
                }}
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

#[derive(Debug, Clone, PartialEq)]
struct RainOutlook {
    today_max_prob: f64,
    today_total_mm: f64,
    today_verdict: RainVerdict,
    today_label: String,
    today_start_at: Option<DateTime<Utc>>,
    next_rain_later: Option<HourlyEstimate>,
}

fn is_rain_hour(hour: &HourlyEstimate) -> bool {
    hour.rain_prob_pct >= 25.0 || hour.rain_mm_expected >= 0.2
}

fn classify_rain_level(prob: f64, mm: f64) -> (RainVerdict, &'static str) {
    if prob >= 60.0 || mm >= 1.0 {
        (RainVerdict::Likely, "Likely")
    } else if prob >= 25.0 || mm >= 0.2 {
        (RainVerdict::Possible, "Possible")
    } else {
        (RainVerdict::Dry, "Dry")
    }
}

fn summarize_rain_outlook(is_raining_now: bool, upcoming: &[HourlyEstimate]) -> RainOutlook {
    let today_hours = &upcoming[..upcoming.len().min(24)];
    let today_max_prob = today_hours
        .iter()
        .map(|h| h.rain_prob_pct)
        .fold(0.0_f64, f64::max);
    let today_total_mm = today_hours.iter().map(|h| h.rain_mm_expected).sum();
    let today_start_at = today_hours
        .iter()
        .find(|h| is_rain_hour(h))
        .map(|h| h.valid_at);

    let (today_verdict, today_label) = if is_raining_now {
        (RainVerdict::Likely, "Raining now".to_string())
    } else {
        let (v, lbl) = classify_rain_level(today_max_prob, today_total_mm);
        (v, lbl.to_string())
    };

    let next_rain_later = if !is_raining_now && today_start_at.is_none() {
        upcoming.iter().skip(24).find(|h| is_rain_hour(h)).cloned()
    } else {
        None
    };

    RainOutlook {
        today_max_prob,
        today_total_mm,
        today_verdict,
        today_label,
        today_start_at,
        next_rain_later,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn mk_hour(offset_hours: i64, prob: f64, mm: f64) -> HourlyEstimate {
        let base = Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap();
        HourlyEstimate {
            valid_at: base + Duration::hours(offset_hours),
            feels_like_c: 18.0,
            feels_like_sun_c: None,
            temp_c: 19.0,
            rain_prob_pct: prob,
            rain_mm_expected: mm,
        }
    }

    #[test]
    fn rain_in_next_24h_sets_start_and_omits_next_rain() {
        let mut hours: Vec<_> = (0..48).map(|i| mk_hour(i, 5.0, 0.0)).collect();
        hours[6].rain_prob_pct = 65.0;
        hours[6].rain_mm_expected = 1.4;
        hours[30].rain_prob_pct = 80.0;

        let outlook = summarize_rain_outlook(false, &hours);
        assert_eq!(outlook.today_verdict, RainVerdict::Likely);
        assert_eq!(outlook.today_start_at, Some(hours[6].valid_at));
        assert!(outlook.next_rain_later.is_none());
    }

    #[test]
    fn dry_next_24h_with_day3_rain_surfaces_next_rain() {
        let mut hours: Vec<_> = (0..72).map(|i| mk_hour(i, 10.0, 0.0)).collect();
        hours[50].rain_prob_pct = 45.0;
        hours[50].rain_mm_expected = 0.6;

        let outlook = summarize_rain_outlook(false, &hours);
        assert_eq!(outlook.today_verdict, RainVerdict::Dry);
        assert_eq!(outlook.today_start_at, None);
        assert_eq!(outlook.next_rain_later, Some(hours[50].clone()));
    }

    #[test]
    fn dry_all_7_days_leaves_next_rain_none() {
        let hours: Vec<_> = (0..168).map(|i| mk_hour(i, 5.0, 0.0)).collect();

        let outlook = summarize_rain_outlook(false, &hours);
        assert_eq!(outlook.today_verdict, RainVerdict::Dry);
        assert_eq!(outlook.today_start_at, None);
        assert!(outlook.next_rain_later.is_none());
    }
}
