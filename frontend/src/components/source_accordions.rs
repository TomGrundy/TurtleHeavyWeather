use leptos::prelude::*;
use thw_core::{SourceKind, SourceReport};

use crate::time::format_local_short;

fn kind_label(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Observation => "Observation",
        SourceKind::Forecast => "Forecast",
        SourceKind::Hybrid => "Obs + Forecast",
    }
}

fn fmt_opt_temp(val: Option<f64>) -> String {
    val.map(|v| format!("{v:.1}°C"))
        .unwrap_or_else(|| "—".to_string())
}

fn fmt_opt_pct(val: Option<f64>) -> String {
    val.map(|v| format!("{v:.0}%"))
        .unwrap_or_else(|| "—".to_string())
}

#[component]
pub fn SourceAccordions(sources: Vec<SourceReport>) -> impl IntoView {
    view! {
        <section class="sources-section">
            <h2>{format!("Individual Weather Sources ({})", sources.len())}</h2>
            {sources.into_iter().map(|src| {
                let title = src.display_name.clone();
                let kind = kind_label(src.kind);
                let feels = fmt_opt_temp(src.current.feels_like_c);
                let temp = fmt_opt_temp(src.current.temp_c);
                let rain_p = fmt_opt_pct(src.current.rain_prob_pct);
                let rain_mm = src
                    .current
                    .rain_mm
                    .map(|m| format!("{m:.1} mm"))
                    .unwrap_or_else(|| "—".to_string());
                let wind = src
                    .current
                    .wind_kmh
                    .map(|w| format!("{w:.0} km/h"))
                    .unwrap_or_else(|| "—".to_string());
                let hum = fmt_opt_pct(src.current.humidity_pct);
                let fetched = format_local_short(src.fetched_at);
                let derived_note = if src.current.feels_like_is_derived {
                    " (Steadman derived)"
                } else {
                    ""
                };
                let summary_text = src
                    .current
                    .summary
                    .clone()
                    .unwrap_or_else(|| "No text summary".to_string());
                let hourly_rows = src.hourly.into_iter().take(8).collect::<Vec<_>>();
                let source_url = src.source_url.clone();

                view! {
                    <details class="source-accordion">
                        <summary>
                            <span>{format!("{title} [{kind}]")}</span>
                            <span class="source-summary-stats">
                                <span>{format!("Feels {feels}")}</span>
                                <span>{format!("Temp {temp}")}</span>
                                <span>{format!("Rain {rain_p}")}</span>
                            </span>
                        </summary>
                        <div class="source-body">
                            <p>
                                <strong>"Summary: "</strong>{summary_text}
                                {format!(" • Weight: {:.2} • Fetched: {fetched}{derived_note}", src.weight)}
                            </p>
                            <p>
                                {format!("Rain: {rain_mm} • Wind: {wind} • Humidity: {hum}")}
                            </p>
                            {source_url.map(|u| view! {
                                <p><a href=u target="_blank" rel="noopener noreferrer">"Source feed"</a></p>
                            })}
                            {if hourly_rows.is_empty() {
                                view! { <div /> }.into_any()
                            } else {
                                view! {
                                    <table class="data-table">
                                        <thead>
                                            <tr>
                                                <th>"Time (Local)"</th>
                                                <th>"Feels Like"</th>
                                                <th>"Temp"</th>
                                                <th>"Rain %"</th>
                                                <th>"Rain mm"</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            {hourly_rows.into_iter().map(|pt| view! {
                                                <tr>
                                                    <td>{format_local_short(pt.valid_at)}</td>
                                                    <td>{fmt_opt_temp(pt.feels_like_c)}</td>
                                                    <td>{fmt_opt_temp(pt.temp_c)}</td>
                                                    <td>{fmt_opt_pct(pt.rain_prob_pct)}</td>
                                                    <td>{pt.rain_mm.map(|m| format!("{m:.1}")).unwrap_or_else(|| "—".into())}</td>
                                                </tr>
                                            }).collect_view()}
                                        </tbody>
                                    </table>
                                }.into_any()
                            }}
                        </div>
                    </details>
                }
            }).collect_view()}
        </section>
    }
}
