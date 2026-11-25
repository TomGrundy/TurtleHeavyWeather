---
id: THW-010
epic: EPIC-04
title: thw-collector scheduled sync CLI orchestrating fetch, aggregate, store, prune, and regress
area: backend
status: done
priority: P0
depends_on: [THW-004, THW-005, THW-007, THW-008, THW-009]
---

## Context
A single deterministic CLI binary (`thw-collector`) must run on a regular interval (via GitHub Actions cron or Home Server timer) to collect Australian weather data across the node mesh.

## Spec & Design Notes
- Reference: [docs/architecture.md](../../docs/architecture.md)
- Implement `backend/crates/thw-collector`:
  - Subcommands / flags: `--nodes data/au_nodes.json --db weather.db --export-dir dist/data`
  - Concurrency control (`tokio` + bounded semaphore) with per-source timeouts & exponential backoff retries on transient HTTP 429/5xx errors.
  - Execution pipeline:
    1. Fetch enabled sources across mesh nodes.
    2. Persist observations & forecast snapshots to SQLite.
    3. Compute `BestEstimate` per node and persist `aggregate_snapshots`.
    4. Prune records older than 7 days.
    5. Run `RegressionEvaluator` and update `regression_metrics`.
    6. Emit structured JSON summary logs (`tracing`) reporting source success/failure counts.

## Acceptance Criteria
- [ ] Partial source outages never abort the run; remaining sources still aggregate and write outputs.
- [ ] Non-zero exit code only if *zero* sources succeed or database write fails.
- [ ] Supports dry-run / fixture replay mode for local testing.

## Test Plan
- End-to-end collector integration test running against local `wiremock` servers and temporary SQLite DB, verifying snapshot persistence, aggregation, pruning, and regression metrics update.

## Execution Log
- *(pending)*
