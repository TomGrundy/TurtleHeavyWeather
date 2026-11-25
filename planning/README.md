# Planning & Spec-Driven Development Guide (`planning/`)

This folder tracks all TurtleHeavyWeather work items using **Jira-esque Markdown tickets** designed for human review and **agent-factory** execution.

## Folder Structure

- [BOARD.md](BOARD.md) — Live status board grouping all tickets by `Backlog`, `Ready`, `In Progress`, `Review`, and `Done`.
- `epics/` — High-level capability groupings (`EPIC-01` .. `EPIC-06`).
- `tickets/` — Atomic, testable work items (`THW-001` .. `THW-016`).

## Ticket Schema

Every ticket contains YAML frontmatter:

```yaml
id: THW-001
epic: EPIC-01
title: Short imperative title
area: backend | frontend | infra | docs
status: backlog | ready | in_progress | review | done
priority: P0 | P1 | P2
depends_on: []
```

Followed by standardized sections:
- `## Context` — Why this ticket exists.
- `## Spec & Design Notes` — Exact crates, files, traits, endpoints, or UI components to create/modify, linking to `docs/`.
- `## Acceptance Criteria` — Checkable objective conditions for completion.
- `## Test Plan` — Automated unit, integration, and fixture tests required.
- `## Execution Log` — Filled in by the implementing agent with files changed and commands verified.

## Agent Factory Loop

1. Pick the highest-priority ticket in `BOARD.md` whose `depends_on` tickets are all `done` (status `ready`).
2. Update the ticket's `status: in_progress` and move it on `BOARD.md`.
3. Implement strictly within the ticket's scope and linked specs in `docs/`.
4. Run all tests and linters (`cargo test`, `cargo clippy`, `cargo fmt --check`, `trunk build`).
5. Update the ticket's checkboxes, `## Execution Log`, and set `status: done` on both the ticket and `BOARD.md`.
