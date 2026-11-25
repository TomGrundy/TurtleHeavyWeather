---
id: THW-016
epic: EPIC-05
title: Regression diagnostics panel, end-to-end integration tests, and API/feature doc sync
area: frontend
status: done
priority: P2
depends_on: [THW-009, THW-015]
---

## Context
To support tuning the aggregation weights using the prior 7 days of forecast-vs-actuals data, expose a clean diagnostics view and ensure full end-to-end test coverage and up-to-date documentation.

## Spec & Design Notes
- Reference: [docs/data-and-regression-spec.md](../../docs/data-and-regression-spec.md), [docs/api-spec.md](../../docs/api-spec.md)
- Implement `frontend/src/components/regression_panel.rs` (accessible via a secondary "Model Accuracy (7d)" tab/route):
  - Displays sortable table of `RegressionMetricRow` comparing `__aggregate__` vs each individual `source_id` across `0-6h`, `6-24h`, and `24-72h` lead buckets (`Temp MAE`, `RMSE`, `Bias`, `Feels-Like MAE`, `Rain Brier Score`, `Hit Rate`).
  - Highlights best-performing source per metric to guide weight optimization.
- Add end-to-end integration test suite (backend fixture sync -> SQLite -> export/server -> frontend data contract validation) and verify all `docs/*.md` reflect implemented endpoints, env vars, and CLI flags.

## Acceptance Criteria
- [ ] Diagnostics view renders 7-day regression table cleanly in both GitHub Pages and Home Server modes.
- [ ] CI runs full backend + frontend + contract integration tests on every PR.
- [ ] `docs/` accurately documents every implemented CLI flag, env var, API route, and weather source.

## Test Plan
- Full workspace `cargo test --workspace`, `trunk build --release`, and headless browser smoke test (Playwright or `wasm-bindgen-test`) verifying location search, hero card, collapsible source accordions, and regression tab.

## Execution Log
- *(pending)*
