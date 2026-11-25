use leptos::prelude::*;
use thw_core::RegressionSummary;

fn fmt_opt(val: Option<f64>) -> String {
    val.map(|v| format!("{v:.2}"))
        .unwrap_or_else(|| "—".to_string())
}

#[component]
pub fn RegressionPanel(summary: Option<RegressionSummary>) -> impl IntoView {
    match summary {
        None => view! {
            <section class="hero-card">
                <h2>"7-Day Forecast vs. Actuals Regression"</h2>
                <p class="hero-meta">
                    "No regression metrics recorded yet. Once hourly syncs accumulate observations over the 7-day rolling window, per-source MAE, RMSE, Bias, and Rain Brier scores will appear here."
                </p>
            </section>
        }
        .into_any(),
        Some(s) => {
            let header_meta = format!(
                "Window: {} days • Computed: {}",
                s.window_days,
                s.computed_at.format("%d %b %Y %H:%M UTC")
            );
            let mut weights = s.suggested_weights.into_iter().collect::<Vec<_>>();
            weights.sort_by(|a, b| b.1.total_cmp(&a.1));

            view! {
                <section class="hero-card">
                    <h2>"7-Day Forecast vs. Actuals Regression"</h2>
                    <p class="hero-meta">{header_meta}</p>
                    <table class="data-table">
                        <thead>
                            <tr>
                                <th>"Source / Model"</th>
                                <th>"Lead"</th>
                                <th>"Samples"</th>
                                <th>"Temp MAE"</th>
                                <th>"Temp RMSE"</th>
                                <th>"Temp Bias"</th>
                                <th>"Feels MAE"</th>
                                <th>"Rain Brier"</th>
                                <th>"Rain Hit"</th>
                            </tr>
                        </thead>
                        <tbody>
                            {s.metrics.into_iter().map(|row| view! {
                                <tr>
                                    <td>{row.display_name}</td>
                                    <td>{row.lead_bucket}</td>
                                    <td>{row.sample_count}</td>
                                    <td>{fmt_opt(row.temp_mae_c)}</td>
                                    <td>{fmt_opt(row.temp_rmse_c)}</td>
                                    <td>{fmt_opt(row.temp_bias_c)}</td>
                                    <td>{fmt_opt(row.feels_like_mae_c)}</td>
                                    <td>{fmt_opt(row.rain_brier_score)}</td>
                                    <td>{fmt_opt(row.rain_hit_rate)}</td>
                                </tr>
                            }).collect_view()}
                        </tbody>
                    </table>

                    {if weights.is_empty() {
                        view! { <div /> }.into_any()
                    } else {
                        view! {
                            <div style="margin-top: 1rem;">
                                <h3>"Suggested Inverse-RMSE Weights"</h3>
                                <p class="hero-meta">
                                    {weights.into_iter().map(|(k, v)| format!("{k}: {v:.3}")).collect::<Vec<_>>().join(" • ")}
                                </p>
                            </div>
                        }.into_any()
                    }}
                </section>
            }
            .into_any()
        }
    }
}
