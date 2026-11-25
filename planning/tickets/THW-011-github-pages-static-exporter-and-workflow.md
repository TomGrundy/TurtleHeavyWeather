---
id: THW-011
epic: EPIC-06
title: GitHub Pages static JSON shard exporter, single-commit state sync, and 60-min workflow
area: infra
status: done
priority: P0
depends_on: [THW-010]
---

## Context
Primary hosting target: deploy TurtleHeavyWeather purely on **GitHub Pages + GitHub Actions** in a **public repository** with a **60-minute** scheduled sync.

## Spec & Design Notes
- Reference: [docs/hosting-feasibility.md](../../docs/hosting-feasibility.md), [docs/api-spec.md](../../docs/api-spec.md), [docs/api-keys-and-secrets.md](../../docs/api-keys-and-secrets.md)
- Implement `StaticExporter` in `thw-store` / `thw-collector`:
  - Emit `locations_index.json` (or `.json.gz`), `nodes/{node_id}.json`, and `regression/summary.json`.
- Create `.github/workflows/sync-and-deploy.yml`:
  - Triggers: `schedule` (`cron: '17 * * * *'` — hourly, staggered off `:00` UTC to minimize queue jitter) + `workflow_dispatch`.
  - Passes optional API secrets (`THW_WEATHERAPI_KEY`, `THW_PIRATEWEATHER_KEY`, `THW_OWM_KEY`, `THW_TOMORROW_KEY`, `THW_USER_AGENT`) from GitHub Repository Secrets.
  - Downloads latest 7-day `weather.db` from a **single-commit orphan branch (`data-state`)** (or GitHub Release asset) so `.git` history never bloats!
  - Runs `thw-collector --export-dir frontend/dist/data`.
  - Force-pushes refreshed `weather.db` back to the orphan `data-state` branch.
  - Builds Leptos WASM app (`trunk build --release`) and deploys `frontend/dist` via `actions/upload-pages-artifact` + `actions/deploy-pages`.

## Acceptance Criteria
- [ ] Exported static bundle conforms strictly to `docs/api-spec.md` schemas.
- [ ] `main` branch is never polluted with automated data commits.
- [ ] README documents the 60-day GitHub Actions cron inactivity caveat and how to keep it active or trigger manually.

## Test Plan
- Validate exported JSON shards against `thw-core` deserialization tests and verify `trunk build --release` bundles `dist/data/` cleanly.

## Execution Log
- *(pending)*
