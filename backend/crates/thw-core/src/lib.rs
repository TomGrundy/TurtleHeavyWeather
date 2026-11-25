pub mod aggregate;
pub mod feels_like;
pub mod geo;
pub mod models;
pub mod regression;

pub use aggregate::{aggregate_node_bundle, classify_rain, AggregationConfig};
pub use feels_like::{compute_apparent_temp_c, ensure_feels_like};
pub use geo::{
    find_nearest_location, find_nearest_node, haversine_km, is_within_australia, search_locations,
    AU_LAT_MAX, AU_LAT_MIN, AU_LON_MAX, AU_LON_MIN,
};
pub use models::*;
pub use regression::{evaluate_regression, RegressionForecastInput};
