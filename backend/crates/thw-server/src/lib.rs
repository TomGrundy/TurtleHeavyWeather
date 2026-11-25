use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use thw_core::{
    find_nearest_node, is_within_australia, search_locations, LocationIndexEntry, MeshNode,
};
use thw_store::WeatherStore;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub db_path: PathBuf,
    pub nodes: Arc<Vec<MeshNode>>,
    pub locations: Arc<Vec<LocationIndexEntry>>,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub timestamp: String,
    pub node_count: usize,
    pub location_count: usize,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(ErrorBody { error: self.1 })).into_response()
    }
}

#[derive(Debug, Deserialize)]
pub struct LocationsQuery {
    pub q: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct WeatherQuery {
    pub node_id: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub postcode: Option<String>,
}

async fn healthz(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        timestamp: Utc::now().to_rfc3339(),
        node_count: state.nodes.len(),
        location_count: state.locations.len(),
    })
}

async fn get_locations(
    State(state): State<AppState>,
    Query(params): Query<LocationsQuery>,
) -> Json<Vec<LocationIndexEntry>> {
    let limit = params.limit.unwrap_or(15).clamp(1, 20_000);
    let q = params.q.unwrap_or_default();
    if q.trim().is_empty() {
        Json(state.locations.iter().take(limit).cloned().collect())
    } else {
        let hits = search_locations(&q, &state.locations, limit)
            .into_iter()
            .cloned()
            .collect();
        Json(hits)
    }
}

async fn get_weather(
    State(state): State<AppState>,
    Query(params): Query<WeatherQuery>,
) -> Result<Response, ApiError> {
    let target_node_id = if let Some(id) = params.node_id {
        id
    } else if let (Some(lat), Some(lon)) = (params.lat, params.lon) {
        if !is_within_australia(lat, lon) {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "Coordinates are outside Australian coverage bounds".into(),
            ));
        }
        let (node, _) = find_nearest_node(lat, lon, &state.nodes).ok_or_else(|| {
            ApiError(
                StatusCode::NOT_FOUND,
                "No Australian mesh nodes loaded".into(),
            )
        })?;
        node.node_id.clone()
    } else if let Some(pc) = params.postcode {
        let hit = state
            .locations
            .iter()
            .find(|l| l.postcode == pc.trim())
            .ok_or_else(|| {
                ApiError(
                    StatusCode::NOT_FOUND,
                    format!("Unknown Australian postcode: {pc}"),
                )
            })?;
        hit.node_id.clone()
    } else {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Provide node_id, postcode, or lat & lon query parameters".into(),
        ));
    };

    let store = WeatherStore::open_read_only(&state.db_path).map_err(|e| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Store error: {e}"),
        )
    })?;

    let bundle = store
        .fetch_latest_node_bundle(&target_node_id)
        .map_err(|e| {
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Query error: {e}"),
            )
        })?
        .ok_or_else(|| {
            ApiError(
                StatusCode::NOT_FOUND,
                format!("No weather bundle stored yet for node {target_node_id}"),
            )
        })?;

    Ok(Json(bundle).into_response())
}

async fn get_regression(State(state): State<AppState>) -> Result<Response, ApiError> {
    let store = WeatherStore::open_read_only(&state.db_path).map_err(|e| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Store error: {e}"),
        )
    })?;

    let summary = store
        .load_regression_summary()
        .map_err(|e| {
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Query error: {e}"),
            )
        })?
        .ok_or_else(|| {
            ApiError(
                StatusCode::NOT_FOUND,
                "No regression metrics computed yet".into(),
            )
        })?;

    Ok(Json(summary).into_response())
}

pub fn build_router(state: AppState, static_dir: Option<PathBuf>) -> Router {
    let api = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/locations", get(get_locations))
        .route("/api/v1/weather", get(get_weather))
        .route("/api/v1/regression", get(get_regression))
        .with_state(state);

    let router = if let Some(dir) = static_dir {
        let index_file = dir.join("index.html");
        api.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index_file)))
    } else {
        api
    };

    router
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}
