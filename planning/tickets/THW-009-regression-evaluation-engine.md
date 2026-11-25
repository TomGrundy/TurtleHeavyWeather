---
id: THW-009
epic: EPIC-03
title: Forecast-vs-actuals 7-day regression evaluator (MAE, RMSE, Bias, Brier score)
area: backend
status: done
priority: P1
depends_on: [THW-007, THW-008]
---

## Context
To tune source weights over time, we need to evaluate how closely both individual sources and the TurtleHeavyWeather Best Estimate matched actual BOM observations over the prior 7 days.

## Spec & Design Notes
- Reference: [docs/data-and-regression-spec.md](../../docs/data-and-regression-spec.md)
- Implement `RegressionEvaluator` in `thw-core/src/regression.rs` + store query in `thw-store`:
  - Select ground truth per `(node_id, valid_at)` using the quality-ranked hierarchy: Tier 1 BOM physical AWS station observation (`bom_obs_json`) within `±30 min` (or nearest station within 25 km), falling back to Tier 2 reanalysis / observation-consensus if missing, with sensor sanity validation.
  - Pair each historical `forecast_snapshots` and `aggregate_snapshots` entry (`valid_at <= now`) to that ground-truth observation.
  - Group by `(subject_id, lead_bucket)` where `lead_bucket` ∈ `{"0-6h", "6-24h", "24-72h"}`.
  - Compute:
    - `temp_mae_c`, `temp_rmse_c`, `temp_bias_c`
    - `feels_like_mae_c`, `feels_like_rmse_c`
    - `rain_brier_score` (`(p/100 - y)^2`, `y = 1` if `rain_mm_1h >= 0.2`) and `rain_hit_rate`
  - Persist materialized summary into `regression_metrics` and provide a CLI/JSON/CSV export hook for offline optimization scripts.

## Acceptance Criteria
- [ ] Computes accurate MAE, RMSE, signed Bias, and Brier score across all lead buckets.
- [ ] Handles nodes/hours lacking an observation gracefully without crashing.
- [ ] Outputs suggested inverse-RMSE weights summary alongside metrics.

## Test Plan
- Synthetic ground-truth unit tests (perfect forecast = 0 error, constant +2°C warm bias = +2.0 bias & 2.0 MAE/RMSE, Brier score verification).

## Execution Log
- *(pending)*
