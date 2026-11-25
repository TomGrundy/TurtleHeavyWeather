---
id: THW-005
epic: EPIC-02
title: Open-Meteo (BOM ACCESS-G + ECMWF + GFS) and MET Norway adapters
area: backend
status: done
priority: P0
depends_on: [THW-003]
---

## Context
Open-Meteo and MET Norway provide free, keyless, global-to-Australian forecasts with explicit multi-model access (including BOM's own ACCESS-G model via Open-Meteo).

## Spec & Design Notes
- Reference: [docs/weather-sources.md](../../docs/weather-sources.md)
- Implement in `thw-sources/src/`:
  1. `OpenMeteoSource`: Configurable model parameter (`bom_access_global`, `ecmwf_ifs025`, `gfs_global`) so each model appears as its own distinct `source_id` in the UI and regression table. Supports **multi-coordinate batching** (chunked e.g. 50 nodes/request) to stay far below fair-use caps.
  2. `MetNorwaySource`: Queries `locationforecast/2.0/compact` with compliant `User-Agent` header (`THW_USER_AGENT`), parses `instant` + `next_1_hours` / `next_6_hours`, respects `Expires` / rate pacing, and derives `feels_like_c` via `thw_core::compute_apparent_temp_c` when omitted.

## Acceptance Criteria
- [ ] Batching reduces Open-Meteo HTTP calls for 100+ nodes to <= 3 requests per model per sync.
- [ ] MET Norway enforces required `User-Agent` and polite request throttling.
- [ ] Each model (`open_meteo_bom_access`, `open_meteo_ecmwf`, `open_meteo_gfs`, `met_no`) emits an independent `SourceReport`.

## Test Plan
- `wiremock` unit tests verifying batch coordinate splitting, JSON parsing, missing-field fallback, and `User-Agent` header enforcement.

## Execution Log
- *(pending)*
