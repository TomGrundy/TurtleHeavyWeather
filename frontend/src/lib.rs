pub mod client;
pub mod components;
pub mod geolocation;

use thw_core::LocationIndexEntry;

pub fn format_location_label(loc: &LocationIndexEntry) -> String {
    format!(
        "{}, {:?} {} (~{:.1} km to node)",
        loc.suburb, loc.state, loc.postcode, loc.distance_km
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use thw_core::{search_locations, StateCode};

    #[test]
    fn search_and_format_smoke() {
        let entries = vec![LocationIndexEntry {
            suburb: "Sydney".into(),
            postcode: "2000".into(),
            state: StateCode::NSW,
            lat: -33.8688,
            lon: 151.2093,
            node_id: "bom-066062".into(),
            distance_km: 1.0,
        }];
        let hits = search_locations("2000", &entries, 5);
        assert_eq!(hits.len(), 1);
        assert!(format_location_label(hits[0]).contains("Sydney, NSW 2000"));
    }
}
