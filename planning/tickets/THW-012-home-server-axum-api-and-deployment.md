---
id: THW-012
epic: EPIC-06
title: Personal NUC (Home Server) thw-server Axum API, static/SSR frontend host, and Docker/systemd/Caddy setup
area: backend
status: done
priority: P1
depends_on: [THW-010]
---

## Context
When sub-hourly scheduling, residential-IP web scraping, or dynamic on-demand suburb queries are needed, host both the Rust backend and Leptos frontend on the user's Home Server.

## Spec & Design Notes
- Reference: [docs/hosting-feasibility.md](../../docs/hosting-feasibility.md), [docs/api-spec.md](../../docs/api-spec.md)
- Implement `backend/crates/thw-server` (`axum` + `tower-http`):
  - Endpoints: `GET /healthz`, `GET /api/v1/locations?q=...`, `GET /api/v1/weather?node_id=...` (or `lat`/`lon`/`postcode`), `GET /api/v1/regression`.
  - Serves compiled `frontend/dist` static files (SPA fallback to `index.html`).
  - Optional built-in background `tokio::time::interval` collector loop (configurable via `--sync-interval-mins` or external `systemd` timer).
  - Read-only SQLite pool (WAL mode) + per-IP rate limiting for on-demand cache misses.
- Provide Home Server deployment assets in `deploy/home-server/`:
  - `Dockerfile` (multi-stage build for `thw-server` + `trunk` frontend)
  - `compose.yaml` + `Caddyfile` (automatic HTTPS / Tailscale notes)
  - `turtleheavyweather.service` + `turtleheavyweather-sync.timer` (`systemd` units)
  - `docs/home-server-setup.md` runbook.

## Acceptance Criteria
- [ ] `thw-server` serves both `/api/v1/*` and the Leptos frontend from a single binary/container.
- [ ] All API endpoints return valid JSON matching `docs/api-spec.md` with proper HTTP status codes (`400` out-of-Australia coords, `404` unknown node).
- [ ] Deployment runbook covers reverse proxy TLS, Tailscale/Cloudflare Tunnel options, and SQLite backup/permissions.

## Test Plan
- `axum::test` / `tower::ServiceExt::oneshot` integration tests for all routes and error cases against a seeded test SQLite DB.

## Execution Log
- *(pending)*
