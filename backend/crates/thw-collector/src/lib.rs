use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use chrono::{Duration, Utc};
use thiserror::Error;
use thw_core::{
    aggregate_node_bundle, evaluate_regression, AggregationConfig, MeshNode, NodeWeatherBundle,
    RegressionForecastInput, RegressionSummary, SourceReport,
};
use thw_sources::bom::{BomFtpXmlSource, BomObsJsonSource, BomWebApiSource};
use thw_sources::keyed::{
    OpenWeatherMapSource, PirateWeatherSource, TomorrowIoSource, WeatherApiSource,
};
use thw_sources::met_no::MetNorwaySource;
use thw_sources::open_meteo::{OpenMeteoModel, OpenMeteoSource};
use thw_sources::scraper::BomHtmlScraperSource;
use thw_sources::{FetchContext, SourceOutput, WeatherSource};
use thw_store::{StoreError, StoredForecastRow, WeatherStore};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::{error, info, warn};

#[derive(Debug, Error)]
pub enum CollectorError {
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("all enabled weather sources failed")]
    AllSourcesFailed,
}

#[derive(Debug, Clone)]
pub struct SyncReport {
    pub sources_attempted: usize,
    pub sources_succeeded: usize,
    pub observations_stored: usize,
    pub bundles_generated: usize,
    pub pruned_forecasts: usize,
    pub pruned_aggregates: usize,
    pub pruned_observations: usize,
    pub regression_rows: usize,
}

pub fn default_sources() -> Vec<Box<dyn WeatherSource>> {
    vec![
        Box::new(BomObsJsonSource::default()),
        Box::new(BomFtpXmlSource::default()),
        Box::new(BomWebApiSource::new()),
        Box::new(OpenMeteoSource::new(OpenMeteoModel::BomAccessGlobal)),
        Box::new(OpenMeteoSource::new(OpenMeteoModel::EcmwfIfs).with_tier2_observations(true)),
        Box::new(OpenMeteoSource::new(OpenMeteoModel::GfsGlobal)),
        Box::new(OpenMeteoSource::new(OpenMeteoModel::IconGlobal)),
        Box::new(MetNorwaySource::default()),
        Box::new(WeatherApiSource::from_env()),
        Box::new(PirateWeatherSource::from_env()),
        Box::new(OpenWeatherMapSource::from_env()),
        Box::new(TomorrowIoSource::from_env()),
        Box::new(BomHtmlScraperSource::default()),
    ]
}

fn to_regression_input(row: StoredForecastRow) -> RegressionForecastInput {
    RegressionForecastInput {
        node_id: row.node_id,
        subject_id: row.subject_id,
        issued_at: row.issued_at,
        valid_at: row.valid_at,
        lead_hours: row.lead_hours,
        temp_c: row.temp_c,
        feels_like_c: row.feels_like_c,
        rain_prob_pct: row.rain_prob_pct,
    }
}

pub fn export_static_shards(
    export_dir: &Path,
    bundles: &[NodeWeatherBundle],
    regression: &RegressionSummary,
    locations_index_src: Option<&Path>,
) -> Result<(), CollectorError> {
    let nodes_dir = export_dir.join("nodes");
    let reg_dir = export_dir.join("regression");
    std::fs::create_dir_all(&nodes_dir)?;
    std::fs::create_dir_all(&reg_dir)?;

    for bundle in bundles {
        let out_path = nodes_dir.join(format!("{}.json", bundle.node.node_id));
        let json = serde_json::to_string(bundle)?;
        std::fs::write(out_path, json)?;
    }

    let reg_json = serde_json::to_string_pretty(regression)?;
    std::fs::write(reg_dir.join("summary.json"), reg_json)?;

    if let Some(src) = locations_index_src {
        if src.exists() {
            let dest = export_dir.join("locations_index.json");
            if src != dest {
                std::fs::copy(src, dest)?;
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run_sync_cycle_with_sources(
    store: &mut WeatherStore,
    nodes: &[MeshNode],
    sources: &[Arc<dyn WeatherSource>],
    agg_cfg: &AggregationConfig,
    export_dir: Option<&Path>,
    locations_index_src: Option<&Path>,
    retention_days: i64,
    window_days: u32,
) -> Result<SyncReport, CollectorError> {
    store.upsert_nodes(nodes)?;

    let ctx = FetchContext::default();
    let sem = Arc::new(Semaphore::new(6));
    let mut join_set = JoinSet::new();

    let mut display_names: HashMap<String, String> = HashMap::new();
    display_names.insert(
        "__aggregate__".to_string(),
        "TurtleHeavyWeather Best Estimate".to_string(),
    );

    let mut sources_attempted = 0usize;
    for src in sources {
        display_names.insert(src.source_id().to_string(), src.display_name().to_string());
        if !src.is_enabled() {
            info!(source_id = src.source_id(), "skipping disabled source");
            continue;
        }
        sources_attempted += 1;
        let permit_sem = Arc::clone(&sem);
        let src_arc = Arc::clone(src);
        let nodes_owned = nodes.to_vec();
        let ctx_owned = ctx.clone();

        join_set.spawn(async move {
            let _permit = permit_sem.acquire_owned().await;
            let id = src_arc.source_id();
            let res = src_arc.fetch_mesh(&nodes_owned, &ctx_owned).await;
            (id, res)
        });
    }

    let mut reports_by_node: HashMap<String, Vec<SourceReport>> = HashMap::new();
    let mut observations = Vec::new();
    let mut sources_succeeded = 0usize;

    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok((
                source_id,
                Ok(SourceOutput {
                    reports,
                    observations: obs,
                }),
            )) => {
                info!(
                    source_id,
                    report_count = reports.len(),
                    obs_count = obs.len(),
                    "source fetch succeeded"
                );
                sources_succeeded += 1;
                for (node_id, report) in reports {
                    reports_by_node.entry(node_id).or_default().push(report);
                }
                observations.extend(obs);
            }
            Ok((source_id, Err(err))) => {
                warn!(source_id, error = %err, "source fetch failed; continuing");
            }
            Err(join_err) => {
                error!(error = %join_err, "source task panicked");
            }
        }
    }

    if sources_attempted > 0 && sources_succeeded == 0 {
        return Err(CollectorError::AllSourcesFailed);
    }

    let obs_count = observations.len();
    if !observations.is_empty() {
        store.upsert_observations(&observations)?;
    }

    let now = Utc::now();
    let mut bundles = Vec::new();
    for node in nodes {
        let Some(node_reports) = reports_by_node.get(&node.node_id) else {
            continue;
        };
        store.upsert_source_reports(&node.node_id, node_reports)?;
        if let Some(bundle) = aggregate_node_bundle(node, node_reports, now, agg_cfg) {
            store.upsert_aggregate_bundle(&bundle, "v1-static")?;
            bundles.push(bundle);
        }
    }

    let cutoff = now - Duration::days(retention_days);
    let prune = store.prune_older_than(cutoff)?;

    let window_since = now - Duration::days(i64::from(window_days));
    let hist_forecasts: Vec<RegressionForecastInput> = store
        .list_historical_forecasts_for_regression(window_since, now)?
        .into_iter()
        .map(to_regression_input)
        .collect();
    let hist_obs = store.list_observations_window(window_since, now)?;
    let regression =
        evaluate_regression(&hist_forecasts, &hist_obs, now, window_days, &display_names);
    store.replace_regression_metrics(&regression)?;

    if let Some(dir) = export_dir {
        export_static_shards(dir, &bundles, &regression, locations_index_src)?;
    }

    Ok(SyncReport {
        sources_attempted,
        sources_succeeded,
        observations_stored: obs_count,
        bundles_generated: bundles.len(),
        pruned_forecasts: prune.forecast_snapshots_deleted,
        pruned_aggregates: prune.aggregate_snapshots_deleted,
        pruned_observations: prune.observations_deleted,
        regression_rows: regression.metrics.len(),
    })
}
