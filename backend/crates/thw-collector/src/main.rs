use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use thw_collector::{default_sources, run_sync_cycle_with_sources};
use thw_core::{AggregationConfig, MeshNode};
use thw_sources::WeatherSource;
use thw_store::WeatherStore;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "thw-collector",
    about = "TurtleHeavyWeather scheduled collector CLI"
)]
struct Cli {
    /// Path to Australian mesh nodes JSON file
    #[arg(long, default_value = "data/au_nodes.json")]
    nodes: PathBuf,

    /// Path to rolling 7-day SQLite database
    #[arg(long, default_value = "weather.db")]
    db: PathBuf,

    /// Optional directory to export static JSON shards for GitHub Pages
    #[arg(long)]
    export_dir: Option<PathBuf>,

    /// Optional path to locations_index.json to copy into export_dir
    #[arg(long)]
    locations_index: Option<PathBuf>,

    /// Rolling retention window in days
    #[arg(long, default_value_t = 7)]
    retention_days: i64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let nodes_raw = std::fs::read_to_string(&cli.nodes)?;
    let nodes: Vec<MeshNode> = serde_json::from_str(&nodes_raw)?;

    let mut store = WeatherStore::open(&cli.db)?;
    let sources: Vec<Arc<dyn WeatherSource>> =
        default_sources().into_iter().map(Arc::from).collect();

    let report = run_sync_cycle_with_sources(
        &mut store,
        &nodes,
        &sources,
        &AggregationConfig::default(),
        cli.export_dir.as_deref(),
        cli.locations_index.as_deref(),
        cli.retention_days,
        cli.retention_days.max(1) as u32,
    )
    .await?;

    info!(?report, "collection cycle complete");
    Ok(())
}
