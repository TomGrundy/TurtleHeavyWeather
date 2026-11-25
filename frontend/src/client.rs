use gloo_net::http::Request;
use thw_core::{LocationIndexEntry, NodeWeatherBundle, RegressionSummary};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportMode {
    StaticJson,
    HomeServerApi,
}

#[derive(Debug, Clone)]
pub struct WeatherDataClient {
    pub mode: TransportMode,
    pub base_url: String,
}

impl Default for WeatherDataClient {
    fn default() -> Self {
        let mode = match option_env!("THW_TRANSPORT") {
            Some("api") => TransportMode::HomeServerApi,
            _ => TransportMode::StaticJson,
        };
        Self {
            mode,
            base_url: String::new(),
        }
    }
}

impl WeatherDataClient {
    pub async fn fetch_locations(&self) -> Result<Vec<LocationIndexEntry>, String> {
        let url = match self.mode {
            TransportMode::StaticJson => format!("{}/data/locations_index.json", self.base_url),
            TransportMode::HomeServerApi => {
                format!("{}/api/v1/locations?limit=20000", self.base_url)
            }
        };
        let resp = Request::get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch locations: {e}"))?;
        if !resp.ok() {
            return Err(format!("Locations HTTP {}", resp.status()));
        }
        resp.json::<Vec<LocationIndexEntry>>()
            .await
            .map_err(|e| format!("Invalid locations JSON: {e}"))
    }

    pub async fn fetch_node_weather(&self, node_id: &str) -> Result<NodeWeatherBundle, String> {
        let url = match self.mode {
            TransportMode::StaticJson => format!("{}/data/nodes/{node_id}.json", self.base_url),
            TransportMode::HomeServerApi => {
                format!("{}/api/v1/weather?node_id={node_id}", self.base_url)
            }
        };
        let resp = Request::get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch weather: {e}"))?;
        if !resp.ok() {
            return Err(format!(
                "Weather data not yet available for {node_id} (HTTP {})",
                resp.status()
            ));
        }
        resp.json::<NodeWeatherBundle>()
            .await
            .map_err(|e| format!("Invalid weather bundle JSON: {e}"))
    }

    pub async fn fetch_weather_by_coords(
        &self,
        lat: f64,
        lon: f64,
        fallback_node_id: &str,
    ) -> Result<NodeWeatherBundle, String> {
        match self.mode {
            TransportMode::StaticJson => self.fetch_node_weather(fallback_node_id).await,
            TransportMode::HomeServerApi => {
                let url = format!("{}/api/v1/weather?lat={lat}&lon={lon}", self.base_url);
                let resp = Request::get(&url)
                    .send()
                    .await
                    .map_err(|e| format!("Failed to fetch weather by coords: {e}"))?;
                if !resp.ok() {
                    return Err(format!("Weather API HTTP {}", resp.status()));
                }
                resp.json::<NodeWeatherBundle>()
                    .await
                    .map_err(|e| format!("Invalid weather bundle JSON: {e}"))
            }
        }
    }

    pub async fn fetch_regression(&self) -> Result<RegressionSummary, String> {
        let url = match self.mode {
            TransportMode::StaticJson => format!("{}/data/regression/summary.json", self.base_url),
            TransportMode::HomeServerApi => format!("{}/api/v1/regression", self.base_url),
        };
        let resp = Request::get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch regression summary: {e}"))?;
        if !resp.ok() {
            return Err(format!("Regression summary HTTP {}", resp.status()));
        }
        resp.json::<RegressionSummary>()
            .await
            .map_err(|e| format!("Invalid regression JSON: {e}"))
    }
}
