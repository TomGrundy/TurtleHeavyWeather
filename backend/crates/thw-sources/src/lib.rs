pub mod bom;
pub mod keyed;
pub mod met_no;
pub mod open_meteo;
pub mod scraper;

use async_trait::async_trait;
use thiserror::Error;
use thw_core::{MeshNode, ObservationRecord, SourceReport};

#[derive(Debug, Error)]
pub enum SourceError {
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("ftp request failed: {0}")]
    Ftp(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("rate budget exceeded for {0}")]
    RateLimited(String),
    #[error("source disabled: {0}")]
    Disabled(String),
}

#[derive(Debug, Clone)]
pub struct SourceOutput {
    pub reports: Vec<(String, SourceReport)>, // (node_id, report)
    pub observations: Vec<ObservationRecord>,
}

impl SourceOutput {
    pub fn empty() -> Self {
        Self {
            reports: Vec::new(),
            observations: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FetchContext {
    pub user_agent: String,
    pub timeout_secs: u64,
}

impl Default for FetchContext {
    fn default() -> Self {
        Self {
            user_agent: std::env::var("THW_USER_AGENT").unwrap_or_else(|_| {
                "TurtleHeavyWeather/0.1 (+https://github.com/turtleheavyweather/turtleheavyweather)"
                    .to_string()
            }),
            timeout_secs: 20,
        }
    }
}

#[async_trait]
pub trait WeatherSource: Send + Sync {
    fn source_id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn default_weight(&self) -> f64;
    fn is_enabled(&self) -> bool {
        true
    }

    async fn fetch_mesh(
        &self,
        nodes: &[MeshNode],
        ctx: &FetchContext,
    ) -> Result<SourceOutput, SourceError>;
}
