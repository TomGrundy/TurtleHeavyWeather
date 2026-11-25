---
id: THW-003
epic: EPIC-02
title: Core domain types, WeatherSource trait, and Steadman feels-like calculator
area: backend
status: done
priority: P0
depends_on: [THW-001, THW-002]
---

## Context
All backend adapters, storage layers, aggregation logic, and the Leptos frontend need a single shared contract in `thw-core` (compiling on both native and `wasm32-unknown-unknown`).

## Spec & Design Notes
- Reference: [docs/architecture.md](../../docs/architecture.md), [docs/api-spec.md](../../docs/api-spec.md)
- Define serializable (`serde`) structs in `thw-core`:
  - `Location`, `MeshNode`, `LocationIndexEntry`
  - `SourceKind` (`Observation`, `Forecast`, `Hybrid`)
  - `WeatherPoint` (`valid_at`, `temp_c`, `feels_like_c`, `feels_like_is_derived`, `rain_prob_pct`, `rain_mm`, `wind_kmh`, `humidity_pct`, `summary`)
  - `SourceReport`, `BestEstimate`, `RainVerdict` (`Dry`, `Possible`, `Likely`, `Raining`), `NodeWeatherBundle`, `RegressionMetricRow`
- Define spatial lookup helper (`find_nearest_node(lat, lon, nodes)`) with Australian bounds validation.
- Implement BOM Australian Apparent Temperature (Steadman formula) fallback `compute_apparent_temp_c(temp_c, rel_humidity_pct, wind_kmh)` for sources that do not supply a native feels-like temperature.
- Define async `WeatherSource` trait (behind non-WASM feature or in `thw-sources` re-exporting `thw-core` types).

## Acceptance Criteria
- [ ] `thw-core` compiles cleanly for both host target and `wasm32-unknown-unknown` (no OS/tokio/sqlite dependencies in `thw-core`).
- [ ] Serialization round-trip matches schemas in `docs/api-spec.md`.
- [ ] Steadman apparent temperature helper matches BOM reference formula outputs within `±0.1°C`.

## Test Plan
- Unit tests for `compute_apparent_temp_c`, `find_nearest_node`, bounds validation, and `serde_json` schema round-trips.

## Execution Log
- *(pending)*
