use std::f64::consts::PI;

use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use gloo_net::http::Request;
use leptos::prelude::*;
use serde::Deserialize;
use wasm_bindgen_futures::{spawn_local, JsFuture};

use crate::time::{format_local_hhmm, local_weekday_name, now_utc};

const RAINVIEWER_API_URL: &str = "https://api.rainviewer.com/public/weather-maps.json";
const OPEN_METEO_ECMWF_LATEST_URL: &str =
    "https://openmeteo.s3.us-west-2.amazonaws.com/data_spatial/ecmwf_ifs/latest.json";
const MAX_FUTURE_FRAMES: usize = 48;

#[cfg(target_arch = "wasm32")]
mod forecast_bridge {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = thwForecastMap, js_name = mount)]
        pub fn mount(container: &JsValue, lat: f64, lon: f64, zoom: u8, valid_time_idx: usize);

        #[wasm_bindgen(js_namespace = thwForecastMap, js_name = setTime)]
        pub fn set_time(valid_time_idx: usize);

        #[wasm_bindgen(js_namespace = thwForecastMap, js_name = setView)]
        pub fn set_view(lat: f64, lon: f64, zoom: u8);

        #[wasm_bindgen(js_namespace = thwForecastMap, js_name = destroy)]
        pub fn destroy();
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod forecast_bridge {
    use wasm_bindgen::JsValue;

    pub fn mount(_container: &JsValue, _lat: f64, _lon: f64, _zoom: u8, _valid_time_idx: usize) {}
    pub fn set_time(_valid_time_idx: usize) {}
    pub fn set_view(_lat: f64, _lon: f64, _zoom: u8) {}
    pub fn destroy() {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RadarMode {
    Future,
    Observed,
}

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

#[derive(Debug, Clone, Deserialize)]
struct OpenMeteoLatest {
    #[serde(default)]
    valid_times: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct ForecastFrameView {
    valid_times_idx: usize,
    time: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
struct ForecastTimeline {
    frames: Vec<ForecastFrameView>,
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

fn parse_om_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%MZ")
                .map(|ndt| ndt.and_utc())
                .ok()
        })
}

fn build_future_frames(valid_times: &[String], now: DateTime<Utc>) -> Vec<ForecastFrameView> {
    let cutoff = now - Duration::minutes(30);
    let future: Vec<ForecastFrameView> = valid_times
        .iter()
        .enumerate()
        .filter_map(|(idx, raw)| {
            let dt = parse_om_timestamp(raw)?;
            (dt >= cutoff).then_some(ForecastFrameView {
                valid_times_idx: idx,
                time: dt,
            })
        })
        .take(MAX_FUTURE_FRAMES)
        .collect();

    if !future.is_empty() {
        return future;
    }

    valid_times
        .iter()
        .enumerate()
        .filter_map(|(idx, raw)| {
            parse_om_timestamp(raw).map(|dt| ForecastFrameView {
                valid_times_idx: idx,
                time: dt,
            })
        })
        .rev()
        .take(MAX_FUTURE_FRAMES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
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

async fn fetch_forecast_timeline() -> Result<ForecastTimeline, String> {
    let resp = Request::get(OPEN_METEO_ECMWF_LATEST_URL)
        .send()
        .await
        .map_err(|e| format!("Forecast manifest request failed: {e}"))?;
    if !resp.ok() {
        return Err(format!("Forecast manifest returned HTTP {}", resp.status()));
    }
    let data: OpenMeteoLatest = resp
        .json()
        .await
        .map_err(|e| format!("Failed parsing forecast manifest: {e}"))?;
    let frames = build_future_frames(&data.valid_times, now_utc());
    if frames.is_empty() {
        return Err("No ECMWF IFS forecast steps currently available.".to_string());
    }
    Ok(ForecastTimeline { frames })
}

#[component]
pub fn RainRadarPanel(lat: f64, lon: f64, location_name: String) -> impl IntoView {
    let (mode, set_mode) = signal(RadarMode::Future);
    let (timeline, set_timeline) = signal(Option::<RadarTimeline>::None);
    let (error_msg, set_error_msg) = signal(Option::<String>::None);
    let (frame_idx, set_frame_idx) = signal(0_usize);

    let (forecast_tl, set_forecast_tl) = signal(Option::<ForecastTimeline>::None);
    let (forecast_err, set_forecast_err) = signal(Option::<String>::None);
    let (forecast_idx, set_forecast_idx) = signal(0_usize);

    let (zoom, set_zoom) = signal(7_u8);
    let (playing, set_playing) = signal(false);
    let (map_mounted, set_map_mounted) = signal(false);
    let map_container = NodeRef::<leptos::html::Div>::new();

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
        spawn_local(async move {
            match fetch_forecast_timeline().await {
                Ok(tl) => {
                    set_forecast_idx.set(0);
                    set_forecast_tl.set(Some(tl));
                    set_forecast_err.set(None);
                }
                Err(err) => set_forecast_err.set(Some(err)),
            }
        });
    });

    Effect::new(move |_| {
        if mode.get() != RadarMode::Future {
            if map_mounted.get_untracked() {
                forecast_bridge::destroy();
                set_map_mounted.set(false);
            }
            return;
        }
        let Some(el) = map_container.get() else {
            return;
        };
        let Some(tl) = forecast_tl.get() else {
            return;
        };
        if tl.frames.is_empty() || map_mounted.get_untracked() {
            return;
        }
        let idx = forecast_idx.get_untracked().min(tl.frames.len() - 1);
        let valid_time_idx = tl.frames[idx].valid_times_idx;
        let z = zoom.get_untracked();
        forecast_bridge::mount(el.as_ref(), lat, lon, z, valid_time_idx);
        set_map_mounted.set(true);
    });

    Effect::new(move |_| {
        let idx = forecast_idx.get();
        if mode.get_untracked() != RadarMode::Future || !map_mounted.get() {
            return;
        }
        if let Some(tl) = forecast_tl.get_untracked() {
            if let Some(frame) = tl.frames.get(idx) {
                forecast_bridge::set_time(frame.valid_times_idx);
            }
        }
    });

    Effect::new(move |_| {
        let z = zoom.get();
        if mode.get_untracked() != RadarMode::Future || !map_mounted.get() {
            return;
        }
        forecast_bridge::set_view(lat, lon, z);
    });

    on_cleanup(|| {
        forecast_bridge::destroy();
    });

    let toggle_play = move |_| {
        let next = !playing.get_untracked();
        set_playing.set(next);
        if next {
            spawn_local(async move {
                loop {
                    let delay = if mode.get_untracked() == RadarMode::Future {
                        1100
                    } else {
                        650
                    };
                    sleep_ms(delay).await;
                    if !playing.get_untracked() {
                        break;
                    }
                    match mode.get_untracked() {
                        RadarMode::Observed => {
                            let count = timeline
                                .get_untracked()
                                .map(|t| t.frames.len())
                                .unwrap_or(0);
                            if count > 1 {
                                set_frame_idx.update(|i| *i = (*i + 1) % count);
                            }
                        }
                        RadarMode::Future => {
                            let count = forecast_tl
                                .get_untracked()
                                .map(|t| t.frames.len())
                                .unwrap_or(0);
                            if count > 1 {
                                set_forecast_idx.update(|i| *i = (*i + 1) % count);
                            }
                        }
                    }
                }
            });
        }
    };

    let switch_mode = move |next_mode: RadarMode| {
        if mode.get_untracked() != next_mode {
            set_playing.set(false);
            set_mode.set(next_mode);
        }
    };

    view! {
        <section class="sources-section radar-section">
            <div class="radar-section-head">
                <h2>{format!("Rain Radar · {location_name}")}</h2>
                <div class="radar-head-controls">
                    <div class="radar-zoom-group">
                        <button
                            type="button"
                            class=move || if mode.get() == RadarMode::Future { "radar-ctrl-btn active" } else { "radar-ctrl-btn" }
                            on:click=move |_| switch_mode(RadarMode::Future)
                        >
                            "Future"
                        </button>
                        <button
                            type="button"
                            class=move || if mode.get() == RadarMode::Observed { "radar-ctrl-btn active" } else { "radar-ctrl-btn" }
                            on:click=move |_| switch_mode(RadarMode::Observed)
                        >
                            "Observed"
                        </button>
                    </div>
                    <div class="radar-zoom-group">
                        <button
                            type="button"
                            class=move || if zoom.get() == 7 { "radar-ctrl-btn active" } else { "radar-ctrl-btn" }
                            on:click=move |_| set_zoom.set(7)
                        >
                            "Metro"
                        </button>
                        <button
                            type="button"
                            class=move || if zoom.get() == 5 { "radar-ctrl-btn active" } else { "radar-ctrl-btn" }
                            on:click=move |_| set_zoom.set(5)
                        >
                            "Regional"
                        </button>
                    </div>
                </div>
            </div>

            <div class="radar-card">
                {move || match mode.get() {
                    RadarMode::Future => {
                        if let Some(err) = forecast_err.get() {
                            return view! { <div class="hero-meta">{err}</div> }.into_any();
                        }
                        let Some(tl) = forecast_tl.get() else {
                            return view! { <div class="hero-meta">"Loading ECMWF IFS precipitation forecast..."</div> }.into_any();
                        };
                        let idx = forecast_idx.get().min(tl.frames.len() - 1);
                        let active = tl.frames[idx].clone();
                        let day_short = local_weekday_name(active.time)
                            .chars()
                            .take(3)
                            .collect::<String>();
                        let time_label = format_local_hhmm(active.time);
                        let frame_count = tl.frames.len();

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
                                                set_forecast_idx.set(v);
                                            }
                                        }
                                    />
                                    <span class="radar-time-badge">
                                        {format!("Forecast · {day_short} {time_label}")}
                                    </span>
                                </div>

                                <div class="radar-viewport">
                                    <div node_ref=map_container class="forecast-map-canvas" />
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
                                    <span class="hero-meta">"Future rain via Open-Meteo (ECMWF IFS)"</span>
                                </div>
                            </div>
                        }.into_any()
                    }
                    RadarMode::Observed => {
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
                                    "https://server.arcgisonline.com/ArcGIS/rest/services/Canvas/World_Dark_Gray_Base/MapServer/tile/{z}/{ty}/{tx}"
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
                                    <span class="hero-meta">"Observed radar via RainViewer / BOM"</span>
                                </div>
                            </div>
                        }.into_any()
                    }
                }}
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn melbourne_tile_coords_sit_inside_center_tile() {
        let coord = lat_lon_to_tile_grid(-37.798, 145.110, 7);
        assert_eq!(coord.z, 7);
        assert!(coord.pin_left_pct > 33.0 && coord.pin_left_pct < 67.0);
        assert!(coord.pin_top_pct > 33.0 && coord.pin_top_pct < 67.0);
    }

    #[test]
    fn builds_future_ecmwf_frames_preserving_manifest_indices() {
        let valid_times = vec![
            "2026-11-26T12:00Z".to_string(),
            "2026-11-26T13:00Z".to_string(),
            "2026-11-26T14:00Z".to_string(),
            "2026-11-26T15:00Z".to_string(),
        ];
        let now = Utc.with_ymd_and_hms(2026, 11, 26, 14, 15, 0).unwrap();
        let frames = build_future_frames(&valid_times, now);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].valid_times_idx, 2);
        assert_eq!(frames[1].valid_times_idx, 3);
    }
}
