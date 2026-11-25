use crate::models::{LocationIndexEntry, MeshNode};

pub const AU_LAT_MIN: f64 = -45.0;
pub const AU_LAT_MAX: f64 = -9.0;
pub const AU_LON_MIN: f64 = 112.0;
pub const AU_LON_MAX: f64 = 155.0;

pub fn is_within_australia(lat: f64, lon: f64) -> bool {
    lat.is_finite()
        && lon.is_finite()
        && (AU_LAT_MIN..=AU_LAT_MAX).contains(&lat)
        && (AU_LON_MIN..=AU_LON_MAX).contains(&lon)
}

pub fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_KM: f64 = 6371.0;
    let phi1 = lat1.to_radians();
    let phi2 = lat2.to_radians();
    let dphi = (lat2 - lat1).to_radians();
    let dlambda = (lon2 - lon1).to_radians();

    let a = (dphi / 2.0).sin().powi(2) + phi1.cos() * phi2.cos() * (dlambda / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    EARTH_RADIUS_KM * c
}

pub fn find_nearest_node(lat: f64, lon: f64, nodes: &[MeshNode]) -> Option<(&MeshNode, f64)> {
    nodes
        .iter()
        .map(|node| (node, haversine_km(lat, lon, node.lat, node.lon)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

pub fn find_nearest_location(
    lat: f64,
    lon: f64,
    locations: &[LocationIndexEntry],
) -> Option<(&LocationIndexEntry, f64)> {
    locations
        .iter()
        .map(|loc| (loc, haversine_km(lat, lon, loc.lat, loc.lon)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// Searches Australian suburbs and 4-digit postcodes, ranking exact/prefix
/// matches ahead of substring matches.
pub fn search_locations<'a>(
    query: &str,
    locations: &'a [LocationIndexEntry],
    limit: usize,
) -> Vec<&'a LocationIndexEntry> {
    let q = query.trim().to_lowercase();
    if q.is_empty() || limit == 0 {
        return Vec::new();
    }

    let mut scored: Vec<(u8, &'a LocationIndexEntry)> = locations
        .iter()
        .filter_map(|entry| {
            let sub = entry.suburb.to_lowercase();
            let pc = entry.postcode.to_lowercase();
            if pc == q || sub == q {
                Some((0, entry))
            } else if pc.starts_with(&q) || sub.starts_with(&q) {
                Some((1, entry))
            } else if sub.contains(&q) || pc.contains(&q) {
                Some((2, entry))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.suburb.cmp(&b.1.suburb))
            .then_with(|| a.1.postcode.cmp(&b.1.postcode))
    });

    scored.into_iter().take(limit).map(|(_, e)| e).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::StateCode;

    #[test]
    fn validates_australian_bounds() {
        assert!(is_within_australia(-33.8688, 151.2093)); // Sydney
        assert!(is_within_australia(-12.4634, 130.8456)); // Darwin
        assert!(is_within_australia(-42.8821, 147.3272)); // Hobart
        assert!(!is_within_australia(51.5072, -0.1276)); // London
        assert!(!is_within_australia(-41.2865, 174.7762)); // Wellington NZ
    }

    #[test]
    fn nearest_node_and_location_search_work() {
        let nodes = vec![
            MeshNode {
                node_id: "bom-066062".into(),
                name: "Sydney".into(),
                state: StateCode::NSW,
                lat: -33.8607,
                lon: 151.205,
                bom_station_id: Some("94768".into()),
                bom_product_id: Some("IDN60901".into()),
                bom_forecast_product: Some("IDN11060".into()),
                is_major_hub: true,
            },
            MeshNode {
                node_id: "bom-086338".into(),
                name: "Melbourne".into(),
                state: StateCode::VIC,
                lat: -37.8255,
                lon: 144.9816,
                bom_station_id: Some("95936".into()),
                bom_product_id: Some("IDV60901".into()),
                bom_forecast_product: Some("IDV10753".into()),
                is_major_hub: true,
            },
        ];

        let (nearest, dist) = find_nearest_node(-33.8861, 151.2111, &nodes).unwrap();
        assert_eq!(nearest.node_id, "bom-066062");
        assert!(dist < 5.0);

        let locs = vec![
            LocationIndexEntry {
                suburb: "Surry Hills".into(),
                postcode: "2010".into(),
                state: StateCode::NSW,
                lat: -33.8861,
                lon: 151.2111,
                node_id: "bom-066062".into(),
                distance_km: 2.9,
            },
            LocationIndexEntry {
                suburb: "South Yarra".into(),
                postcode: "3141".into(),
                state: StateCode::VIC,
                lat: -37.838,
                lon: 144.991,
                node_id: "bom-086338".into(),
                distance_km: 1.6,
            },
        ];

        let by_pc = search_locations("2010", &locs, 10);
        assert_eq!(by_pc.len(), 1);
        assert_eq!(by_pc[0].suburb, "Surry Hills");

        let by_prefix = search_locations("sur", &locs, 10);
        assert_eq!(by_prefix[0].postcode, "2010");
    }

    #[test]
    fn curated_au_locations_index_maps_all_capital_postcodes() {
        let nodes_raw = include_str!("../../../../data/au_nodes.json");
        let locs_raw = include_str!("../../../../data/locations_index.json");
        let nodes: Vec<MeshNode> = serde_json::from_str(nodes_raw).unwrap();
        let locs: Vec<LocationIndexEntry> = serde_json::from_str(locs_raw).unwrap();

        assert!(nodes.len() >= 40);
        assert!(locs.len() > 15_000);

        for pc in [
            "2000", "3000", "4000", "5000", "6000", "7000", "0800", "2600",
        ] {
            let matches = search_locations(pc, &locs, 5);
            assert!(!matches.is_empty(), "expected postcode {pc} in index");
        }
    }
}
