---
id: THW-008
epic: EPIC-04
title: Lightweight SQLite 7-day rolling data store and automated retention pruning
area: backend
status: done
priority: P0
depends_on: [THW-003]
---

## Context
The system intentionally avoids long-term archival storage, needing only a lightweight data store for current forecasts and ~7 days of historical forecast snapshots and observations for regression analysis.

## Spec & Design Notes
- Reference: [docs/data-and-regression-spec.md](../../docs/data-and-regression-spec.md)
- Implement `thw-store` using `sqlx` (SQLite, WAL mode) or `rusqlite`:
  - Versioned SQLite migrations creating `nodes`, `forecast_snapshots`, `aggregate_snapshots`, `observations`, and `regression_metrics`.
  - Idempotent upsert methods for nodes, snapshots, and observations.
  - `prune_older_than(days: i64)` deleting expired rows (`valid_at < now - 7d` / `observed_at < now - 7d`) + periodic incremental vacuum/WAL checkpoint.

## Acceptance Criteria
- [ ] Fresh database initializes automatically via embedded migrations.
- [ ] Re-inserting identical `(node_id, source_id, issued_at, valid_at)` snapshots is idempotent.
- [ ] Pruning removes rows older than 7 days while preserving active future forecasts and recent 7-day history.

## Test Plan
- Integration tests against `:memory:` / temp-file SQLite verifying migrations, upserts, query filters, and 7-day retention pruning boundary cases.

## Execution Log
- *(pending)*
