# TurtleHeavyWeather — Work Board

> **Last Updated:** 2026-11-26
> **Current Phase:** All 6 build waves implemented (`THW-001` through `THW-016`); running full workspace verification.

## Status Summary

### Ready
*(none)*

### Backlog
*(none)*

### In Progress
*(none)*

### Review
*(none)*

### Done
- [THW-001](tickets/THW-001-monorepo-workspace-and-ci.md) — `[P0]` `[infra/backend]` Initialize Rust workspace, Leptos crate skeleton, and GitHub Actions PR CI
- [THW-002](tickets/THW-002-au-suburb-postcode-index.md) — `[P0]` `[backend/data]` Curate Australian suburb/postcode spatial index and representative station/hub mesh
- [THW-003](tickets/THW-003-core-domain-and-source-trait.md) — `[P0]` `[backend]` Core domain types, `WeatherSource` trait, and Steadman feels-like calculator
- [THW-004](tickets/THW-004-source-bom-ftp-and-json.md) — `[P0]` `[backend]` BOM adapters: Public JSON observations, Anonymous FTP XML forecasts, and optional web JSON API
- [THW-005](tickets/THW-005-source-open-meteo-and-metno.md) — `[P0]` `[backend]` Open-Meteo (BOM ACCESS-G + ECMWF + GFS) and MET Norway adapters
- [THW-006](tickets/THW-006-source-keyed-apis-and-scrapers.md) — `[P1]` `[backend]` Optional free-tier keyed APIs and robots/ToS-compliant Australian weather scrapers
- [THW-007](tickets/THW-007-aggregation-best-estimate-engine.md) — `[P0]` `[backend]` Weighted Best-Estimate aggregation engine (Feels-Like, Temp, Rain verdict, spread)
- [THW-008](tickets/THW-008-lightweight-7day-store-and-retention.md) — `[P0]` `[backend]` Lightweight SQLite 7-day rolling data store and automated retention pruning
- [THW-009](tickets/THW-009-regression-evaluation-engine.md) — `[P1]` `[backend]` Forecast-vs-actuals 7-day regression evaluator (MAE, RMSE, Bias, Brier score)
- [THW-010](tickets/THW-010-collector-cli-and-sync-job.md) — `[P0]` `[backend]` `thw-collector` scheduled sync CLI orchestrating fetch, aggregate, store, prune, and regress
- [THW-011](tickets/THW-011-github-pages-static-exporter-and-workflow.md) — `[P0]` `[infra]` GitHub Pages static JSON shard exporter, single-commit state sync, and 60-min workflow
- [THW-012](tickets/THW-012-home-server-axum-api-and-deployment.md) — `[P1]` `[backend/infra]` Personal NUC (Home Server) `thw-server` Axum API, embedded/static frontend host, and Docker/systemd/Caddy setup
- [THW-013](tickets/THW-013-leptos-app-shell-and-location-picker.md) — `[P0]` `[frontend]` Leptos app shell, `WeatherDataClient` transport abstraction, and suburb/postcode search
- [THW-014](tickets/THW-014-browser-geolocation-integration.md) — `[P0]` `[frontend]` Browser `navigator.geolocation` integration and nearest-node Australian bounds handling
- [THW-015](tickets/THW-015-aggregate-hero-card-and-source-accordions.md) — `[P0]` `[frontend]` Best-Estimate Hero Card (Feels-Like, Temp, Rain) and collapsible per-source accordions
- [THW-016](tickets/THW-016-regression-diagnostics-view-and-e2e-tests.md) — `[P2]` `[frontend/qa]` Regression diagnostics panel, end-to-end integration tests, and API/feature doc sync
