---
id: THW-007
epic: EPIC-03
title: Weighted Best-Estimate aggregation engine (Feels-Like, Temp, Rain verdict, spread)
area: backend
status: done
priority: P0
depends_on: [THW-003, THW-004, THW-005]
---

## Context
Users want one clear, trustworthy answer first ("What does it feel like, what's the temperature, and is it going to rain?") synthesized from all available sources before inspecting individual sources.

## Spec & Design Notes
- Reference: [docs/architecture.md](../../docs/architecture.md)
- Implement `AggregateEngine` in `thw-core/src/aggregate.rs`:
  - Load configurable source weights (`weights.toml` or defaults prioritizing BOM + BOM ACCESS + ECMWF).
  - Filter stale/invalid readings; normalize weights over *available* sources for that timestamp.
  - Compute weighted mean for `feels_like_c`, `temp_c`, `rain_prob_pct`, and `rain_mm_expected`.
  - Compute spread (`min..max`, standard deviation) and `confidence` (`high` / `medium` / `low` based on source count & inter-model agreement).
  - Synthesize `RainVerdict`:
    - `Raining` if live observation `rain_mm_1h >= 0.2`
    - `Likely` if consensus `rain_prob_pct >= 60` or `rain_mm_expected >= 1.0`
    - `Possible` if `rain_prob_pct >= 25` or `rain_mm_expected >= 0.2`
    - `Dry` otherwise.

## Acceptance Criteria
- [ ] Handles partial source availability (e.g., 2 of 6 sources down) without NaN or weight drift.
- [ ] Produces deterministic `BestEstimate` for both current conditions and hourly forecast horizon.
- [ ] Weights are externalized so future regression tuning requires config changes only.

## Test Plan
- Unit tests covering consensus calculation, missing/stale source exclusion, outlier spread, and all 4 `RainVerdict` thresholds.

## Execution Log
- *(pending)*
