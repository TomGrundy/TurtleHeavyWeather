# Backend (`backend/`)

Planned Rust Cargo workspace powering data collection, Australian spatial mapping, aggregation, lightweight 7-day persistence, regression analysis, and optional HTTP serving.

> **Status:** Planning phase. See [planning/BOARD.md](../planning/BOARD.md) and [docs/architecture.md](../docs/architecture.md).

## Planned Workspace Crates

| Crate | Responsibility | Ticket |
| :--- | :--- | :--- |
| `thw-core` | Domain models (`WeatherReport`, `BestEstimate`, `Location`), Australian suburb/postcode Haversine index, source normalization, weighted aggregation engine, regression scorer | `THW-002`, `THW-003`, `THW-007`, `THW-009` |
| `thw-sources` | Async `WeatherSource` trait + adapters for BOM (FTP/JSON), Open-Meteo (BOM ACCESS + global models), MET Norway, optional keyed APIs, and compliant scrapers | `THW-004`, `THW-005`, `THW-006` |
| `thw-store` | Lightweight SQLite 7-day rolling store + automatic pruning + static JSON shard exporter for GitHub Pages | `THW-008`, `THW-011` |
| `thw-collector` | CLI binary (`thw-collect`) executing one full fetch-aggregate-regress-prune-export cycle (invoked by GitHub Actions or Home Server timer) | `THW-010` |
| `thw-server` | Optional `axum` REST API + static/SSR host for Home Server deployment (on-demand location cache, regression endpoints) | `THW-012` |

## Quality & CI Requirements

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --workspace` on every PR (`THW-001`).
- All external HTTP/FTP adapters must be tested against deterministic recorded fixtures (`wiremock` / local test vectors) so CI never depends on live third-party uptime.
