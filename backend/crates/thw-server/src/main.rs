use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use thw_collector::{default_sources, run_sync_cycle_with_sources};
use thw_core::{AggregationConfig, LocationIndexEntry, MeshNode};
use thw_server::{build_router, AppState};
use thw_sources::WeatherSource;
use thw_store::WeatherStore;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "thw-server", about = "TurtleHeavyWeather Home Server (NUC) API & UI host")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:8080", env = "THW_BIND_ADDR")]
    bind: SocketAddr,

    #[arg(long, default_value = "weather.db", env = "THW_DB_PATH")]
    db: PathBuf,

    #[arg(long, default_value = "data/au_nodes.json", env = "THW_NODES_PATH")]
    nodes: PathBuf,

    #[arg(long, default_value = "data/locations_index.json", env = "THW_LOCATIONS_PATH")]
    locations: PathBuf,

    #[arg(long, env = "THW_STATIC_DIR")]
    static_dir: Option<PathBuf>,

    /// If > 0, runs background collector sync every N minutes
    #[arg(long, default_value_t = 60, env = "THW_SYNC_INTERVAL_MINS")]
    sync_interval_mins: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let nodes: Vec<MeshNode> = serde_json::from_str(&std::fs::read_to_string(&cli.nodes)?)?;
    let locations: Vec<LocationIndexEntry> =
        serde_json::from_str(&std::fs::read_to_string(&cli.locations)?)?;

    // Ensure DB exists and migrations are applied before serving read-only queries.
    {
        let mut store = WeatherStore::open(&cli.db)?;
        store.upsert_nodes(&nodes)?;
    }

    if cli.sync_interval_mins > 0 {
        let db_path = cli.db.clone();
        let nodes_bg = nodes.clone();
        let interval_mins = cli.sync_interval_mins;
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(interval_mins * 60));
            let sources: Vec<Arc<dyn WeatherSource>> =
                default_sources().into_iter().map(Arc::from).collect();
            loop {
                ticker.tick().await;
                match WeatherStore::open(&db_path) {
                    Ok(mut store) => {
                        match run_sync_cycle_with_sources(
                            &mut store,
                            &nodes_bg,
                            &sources,
                            &AggregationConfig::default(),
                            None,
                            None,
                            7,
                            7,
                        )
                        .await
                        {
                            Ok(rep) => info!(?rep, "background NUC sync complete"),
                            Err(e) => error!(error = %e, "background NUC sync failed"),
                        }
                    }
                    Err(e) => error!(error = %e, "failed to open SQLite DB for background sync"),
                }
            }
        });
    }

    let state = AppState {
        db_path: cli.db,
        nodes: Arc::new(nodes),
        locations: Arc::new(locations),
    };

    let app = build_router(state, cli.static_dir);
    let listener = tokio::net::TcpListener::bind(cli.bind).await?;
    info!(addr = %cli.bind, "thw-server listening");
    axum::serve(listener, app).await?;
    Ok(())
}
