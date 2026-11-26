use std::f64::consts::PI;

use chrono::{DateTime, Utc};
use gloo_net::http::Request;
use leptos::prelude::*;
use serde::Deserialize;
use wasm_bindgen_futures::{spawn_local, JsFuture};

use crate::time::format_local_hhmm;

const RAINVIEWER_API_URL: &str = "https://api.rainviewer.com/public/weather-maps.json";

#[derive(Debug, Clone, Deserialize)]
struct RainViewerResponse {
    host: String,
    radar: RainViewerRadar,
}

#[derive(Debug, Clone, Deserialize)]
struct RainViewerRadar {
    #[serde(default)]
    past: Vec<RainViewerFrame>,
    #[serde(default)]
    nowcast: Vec<RainViewerFrame>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct RainViewerFrame {
    time: i64,
    path: String,
}

#[derive(Debug, Clone, PartialEq)]
struct RadarTimeline {
    host: String,
    frames: Vec<RadarFrameView>,
}

#[derive(Debug, Clone, PartialEq)]
struct RadarFrameView {
    time: DateTime<Utc>,
    path: String,
    is_nowcast: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct TileCoord {
    z: u8,
    cx: i64,
    cy: i64,
    pin_left_pct: f64,
    pin_top_pct: f64,
}

fn lat_lon_to_tile_grid(lat: f64, lon: f64, z: u8) -> TileCoord {
    let n = (1_u64 << z) as f64;
    let clamped_lat = lat.clamp(-85.0511, 85.0511);
    let tx_f = ((lon + 180.0) / 360.0) * n;
    let lat_rad = clamped_lat.to_radians();
    let ty_f = (1.0 - lat_rad.tan().asinh() / PI) / 2.0 * n;

    let cx = tx_f.floor() as i64;
    let cy = ty_f.floor() as i64;
    let fx = tx_f - cx as f64;
    let fy = ty_f - cy as f64;

    TileCoord {
        z,
        cx,
        cy,
        pin_left_pct: ((1.0 + fx) / 3.0) * 100.0,
        pin_top_pct: ((1.0 + fy) / 3.0) * 100.0,
    }
}

async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = JsFuture::from(promise).await;
}

async fn fetch_radar_timeline() -> Result<RadarTimeline, String> {
    let resp = Request::get(RAINVIEWER_API_URL)
        .send()
        .await
        .map_err(|e| format!("Radar request failed: {e}"))?;
    if !resp.ok() {
        return Err(format!("Radar API returned HTTP {}", resp.status()));
    }
    let data: RainViewerResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed parsing radar JSON: {e}"))?;

    let mut frames = Vec::new();
    for f in data.radar.past {
        if let Some(dt) = DateTime::from_timestamp(f.time, 0) {
            frames.push(RadarFrameView {
                time: dt,
                path: f.path,
                is_nowcast: false,
            });
        }
    }
    for f in data.radar.nowcast {
        if let Some(dt) = DateTime::from_timestamp(f.time, 0) {
            frames.push(RadarFrameView {
                time: dt,
                path: f.path,
                is_nowcast: true,
            });
        }
    }

    Ok(RadarTimeline {
        host: data.host,
        frames,
    })
}

#[component]
pub fn RainRadarPanel(lat: f64, lon: f64, location_name: String) -> impl IntoView {
    let (timeline, set_timeline) = signal(Option::<RadarTimeline>::None);
    let (error_msg, set_error_msg) = signal(Option::<String>::None);
    let (frame_idx, set_frame_idx) = signal(0_usize);
    let (zoom, set_zoom) = signal(8_u8);
    let (playing, set_playing) = signal(false);

    Effect::new(move |_| {
        spawn_local(async move {
            match fetch_radar_timeline().await {
                Ok(tl) => {
                    let latest_past = tl
                        .frames
                        .iter()
                        .rposition(|f| !f.is_nowcast)
                        .unwrap_or_else(|| tl.frames.len().saturating_sub(1));
                    set_frame_idx.set(latest_past);
                    set_timeline.set(Some(tl));
                    set_error_msg.set(None);
                }
                Err(err) => set_error_msg.set(Some(err)),
            }
        });
    });

    let toggle_play = move |_| {
        let next = !playing.get_untracked();
        set_playing.set(next);
        if next {
            spawn_local(async move {
                loop {
                    sleep_ms(650).await;
                    if !playing.get_untracked() {
                        break;
                    }
                    let count = timeline
                        .get_untracked()
                        .map(|t| t.frames.len())
                        .unwrap_or(0);
                    if count > 1 {
                        set_frame_idx.update(|i| *i = (*i + 1) % count);
                    }
                }
            });
        }
    };

    view! {
        <section class="sources-section radar-section">
            <div class="radar-section-head">
                <h2>{format!("Rain Radar · {location_name}")}</h2>
                <div class="radar-zoom-group">
                    <button
                        type="button"
                        class=move || if zoom.get() == 8 { "radar-ctrl-btn active" } else { "radar-ctrl-btn" }
                        on:click=move |_| set_zoom.set(8)
                    >
                        "Metro"
                    </button>
                    <button
                        type="button"
                        class=move || if zoom.get() == 6 { "radar-ctrl-btn active" } else { "radar-ctrl-btn" }
                        on:click=move |_| set_zoom.set(6)
                    >
                        "Regional"
                    </button>
                </div>
            </div>

            <div class="radar-card">
                {move || {
                    if let Some(err) = error_msg.get() {
                        return view! { <div class="hero-meta">{err}</div> }.into_any();
                    }
                    let Some(tl) = timeline.get() else {
                        return view! { <div class="hero-meta">"Loading live Australian rain radar..."</div> }.into_any();
                    };
                    if tl.frames.is_empty() {
                        return view! { <div class="hero-meta">"No radar frames currently available."</div> }.into_any();
                    }

                    let idx = frame_idx.get().min(tl.frames.len() - 1);
                    let active_frame = tl.frames[idx].clone();
                    let z = zoom.get();
                    let grid = lat_lon_to_tile_grid(lat, lon, z);
                    let tile_max = 1_i64 << z;
                    let host = tl.host.clone();
                    let frame_path = active_frame.path.clone();

                    let mut tiles = Vec::with_capacity(9);
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let tx = (grid.cx + dx).rem_euclid(tile_max);
                            let ty = (grid.cy + dy).clamp(0, tile_max - 1);
                            let base_url = format!(
                                "https://basemaps.cartocdn.com/dark_all/{z}/{tx}/{ty}.png"
                            );
                            let radar_url = format!(
                                "{host}{frame_path}/256/{z}/{tx}/{ty}/2/1_1.png"
                            );
                            tiles.push((base_url, radar_url));
                        }
                    }

                    let time_label = format_local_hhmm(active_frame.time);
                    let kind_tag = if active_frame.is_nowcast { "Nowcast" } else { "Radar" };
                    let frame_count = tl.frames.len();
                    let pin_style = format!(
                        "left: {:.2}%; top: {:.2}%;",
                        grid.pin_left_pct, grid.pin_top_pct
                    );

                    view! {
                        <div>
                            <div class="radar-controls">
                                <button
                                    type="button"
                                    class="radar-ctrl-btn"
                                    on:click=toggle_play
                                >
                                    {move || if playing.get() { "Pause" } else { "Play" }}
                                </button>
                                <input
                                    type="range"
                                    class="radar-scrubber"
                                    min="0"
                                    max=(frame_count - 1).to_string()
                                    prop:value=idx.to_string()
                                    on:input=move |ev| {
                                        set_playing.set(false);
                                        if let Ok(v) = event_target_value(&ev).parse::<usize>() {
                                            set_frame_idx.set(v);
                                        }
                                    }
                                />
                                <span class="radar-time-badge">
                                    {format!("{kind_tag} · {time_label}")}
                                </span>
                            </div>

                            <div class="radar-viewport">
                                <div class="radar-grid">
                                    {tiles.into_iter().map(|(base_url, radar_url)| {
                                        view! {
                                            <div class="radar-tile">
                                                <img src=base_url alt="" loading="lazy" />
                                                <img src=radar_url alt="" class="radar-overlay-img" />
                                            </div>
                                        }
                                    }).collect_view()}
                                    <div class="radar-pin" style=pin_style title=location_name.clone() />
                                </div>
                            </div>

                            <div class="radar-footer">
                                <div class="radar-legend">
                                    <span class="legend-item">
                                        <span class="radar-swatch radar-swatch-light"></span>
                                        "Light"
                                    </span>
                                    <span class="legend-item">
                                        <span class="radar-swatch radar-swatch-mod"></span>
                                        "Moderate"
                                    </span>
                                    <span class="legend-item">
                                        <span class="radar-swatch radar-swatch-heavy"></span>
                                        "Heavy"
                                    </span>
                                </div>
                                <span class="hero-meta">"Radar via RainViewer / BOM"</span>
                            </div>
                        </div>
                    }.into_any()
                }}
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn melbourne_tile_coords_sit_inside_center_tile() {
        let coord = lat_lon_to_tile_grid(-37.798, 145.110, 8);
        assert_eq!(coord.z, 8);
        assert!(coord.pin_left_pct > 33.0 && coord.pin_left_pct < 67.0);
        assert!(coord.pin_top_pct > 33.0 && coord.pin_top_pct < 67.0);
    }
}
