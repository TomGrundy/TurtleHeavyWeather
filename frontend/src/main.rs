use leptos::prelude::*;
use leptos::task::spawn_local;
use thw_core::{
    find_nearest_location, is_within_australia, search_locations, LocationIndexEntry,
    NodeWeatherBundle, RegressionSummary,
};
use thw_frontend::client::WeatherDataClient;
use thw_frontend::components::{BestEstimateCard, RegressionPanel, SourceAccordions};
use thw_frontend::geolocation::request_browser_location;

const STORAGE_KEY_LOCATION: &str = "thw_selected_location_v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveTab {
    Weather,
    Regression,
}

fn load_saved_location() -> Option<LocationIndexEntry> {
    let window = web_sys::window()?;
    let storage = window.local_storage().ok()??;
    let raw = storage.get_item(STORAGE_KEY_LOCATION).ok()??;
    serde_json::from_str(&raw).ok()
}

fn save_location(loc: &LocationIndexEntry) {
    if let Some(window) = web_sys::window() {
        if let Ok(Some(storage)) = window.local_storage() {
            if let Ok(json) = serde_json::to_string(loc) {
                let _ = storage.set_item(STORAGE_KEY_LOCATION, &json);
            }
        }
    }
}

#[component]
fn App() -> impl IntoView {
    let client = WeatherDataClient::default();
    let (active_tab, set_active_tab) = signal(ActiveTab::Weather);
    let (locations, set_locations) = signal(Vec::<LocationIndexEntry>::new());
    let (query, set_query) = signal(String::new());
    let (selected, set_selected) = signal(load_saved_location());
    let (bundle, set_bundle) = signal(Option::<NodeWeatherBundle>::None);
    let (regression, set_regression) = signal(Option::<RegressionSummary>::None);
    let (status_msg, set_status_msg) = signal(Option::<String>::None);
    let (geo_busy, set_geo_busy) = signal(false);

    let init_client = client.clone();
    Effect::new(move |_| {
        let c = init_client.clone();
        spawn_local(async move {
            match c.fetch_locations().await {
                Ok(list) => {
                    if selected.get_untracked().is_none() {
                        if let Some(default_loc) = list
                            .iter()
                            .find(|l| {
                                l.postcode == "2000" && l.suburb.eq_ignore_ascii_case("Sydney")
                            })
                            .or_else(|| list.first())
                            .cloned()
                        {
                            set_selected.set(Some(default_loc));
                        }
                    }
                    set_locations.set(list);
                }
                Err(err) => set_status_msg.set(Some(err)),
            }
            if let Ok(reg) = c.fetch_regression().await {
                set_regression.set(Some(reg));
            }
        });
    });

    let weather_client = client.clone();
    Effect::new(move |_| {
        if let Some(loc) = selected.get() {
            let c = weather_client.clone();
            let node_id = loc.node_id.clone();
            spawn_local(async move {
                match c.fetch_node_weather(&node_id).await {
                    Ok(data) => {
                        set_bundle.set(Some(data));
                        set_status_msg.set(None);
                    }
                    Err(err) => set_status_msg.set(Some(err)),
                }
            });
        }
    });

    let suggestions = Memo::new(move |_| {
        let q = query.get();
        let all = locations.get();
        search_locations(&q, &all, 12)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
    });

    let on_use_my_location = move |_| {
        set_geo_busy.set(true);
        set_status_msg.set(None);
        request_browser_location(move |res| {
            set_geo_busy.set(false);
            match res {
                Ok(coords) => {
                    if !is_within_australia(coords.lat, coords.lon) {
                        set_status_msg.set(Some(
                            "Detected browser location is outside Australia; please select an Australian suburb or postcode manually.".into(),
                        ));
                        return;
                    }
                    let all = locations.get_untracked();
                    if let Some((nearest, dist_km)) =
                        find_nearest_location(coords.lat, coords.lon, &all)
                    {
                        let mut chosen = nearest.clone();
                        chosen.distance_km = (dist_km * 10.0).round() / 10.0;
                        save_location(&chosen);
                        set_selected.set(Some(chosen));
                        set_query.set(String::new());
                    } else {
                        set_status_msg.set(Some(
                            "Locations index is still loading; please try again in a moment."
                                .into(),
                        ));
                    }
                }
                Err(err) => set_status_msg.set(Some(err)),
            }
        });
    };

    view! {
        <div class="app-shell">
            <header class="topbar">
                <div class="brand">
                    <h1>"TurtleHeavyWeather"</h1>
                    <p>"Australian Multi-Source Consensus Weather"</p>
                </div>
                <nav class="nav-tabs">
                    <button
                        class=move || if active_tab.get() == ActiveTab::Weather { "tab-btn active" } else { "tab-btn" }
                        on:click=move |_| set_active_tab.set(ActiveTab::Weather)
                    >
                        "Forecast"
                    </button>
                    <button
                        class=move || if active_tab.get() == ActiveTab::Regression { "tab-btn active" } else { "tab-btn" }
                        on:click=move |_| set_active_tab.set(ActiveTab::Regression)
                    >
                        "Model Accuracy (7d)"
                    </button>
                </nav>
            </header>

            {move || match active_tab.get() {
                ActiveTab::Regression => view! {
                    <RegressionPanel summary=regression.get() />
                }.into_any(),
                ActiveTab::Weather => view! {
                    <div>
                        <section class="search-panel">
                            <div class="search-row">
                                <div class="search-input-wrap">
                                    <input
                                        class="search-input"
                                        type="text"
                                        placeholder="Search Australian suburb or postcode (e.g. Surry Hills or 2000)..."
                                        prop:value=move || query.get()
                                        on:input=move |ev| set_query.set(event_target_value(&ev))
                                    />
                                </div>
                                <button
                                    class="geo-btn"
                                    disabled=move || geo_busy.get()
                                    on:click=on_use_my_location
                                >
                                    {move || if geo_busy.get() { "Locating..." } else { "Use My Location" }}
                                </button>
                            </div>

                            {move || {
                                let items = suggestions.get();
                                if items.is_empty() {
                                    view! { <div /> }.into_any()
                                } else {
                                    view! {
                                        <ul class="suggestions">
                                            {items.into_iter().map(|entry| {
                                                let chosen = entry.clone();
                                                let label = format!("{}, {:?} {}", entry.suburb, entry.state, entry.postcode);
                                                let node_hint = entry.node_id.clone();
                                                view! {
                                                    <li>
                                                        <button
                                                            class="suggestion-item"
                                                            on:click=move |_| {
                                                                save_location(&chosen);
                                                                set_selected.set(Some(chosen.clone()));
                                                                set_query.set(String::new());
                                                            }
                                                        >
                                                            <span>{label}</span>
                                                            <span class="hero-meta">{node_hint}</span>
                                                        </button>
                                                    </li>
                                                }
                                            }).collect_view()}
                                        </ul>
                                    }.into_any()
                                }
                            }}

                            {move || status_msg.get().map(|msg| view! {
                                <div class="status-banner">{msg}</div>
                            })}
                        </section>

                        {move || bundle.get().map(|b| {
                            let sources = b.sources.clone();
                            let sel = selected.get();
                            view! {
                                <div>
                                    <BestEstimateCard bundle=b selected_location=sel />
                                    <SourceAccordions sources=sources />
                                </div>
                            }
                        })}
                    </div>
                }.into_any(),
            }}
        </div>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(|| view! { <App /> });
}
