# TurtleHeavyWeather

An Australian weather aggregation and forecast-verification monorepo built with a **Rust** backend, a **Leptos** (Rust/WASM) web frontend, and **spec-driven development** tracking for iterative agent-factory execution.

## Goals

- Collect Australian weather forecasts and observations on a regular schedule from all accessible public APIs, feeds, and compliant web-scrapable reports.
- Collate disparate sources into a weighted **Best Estimate** per Australian suburb or postcode (plus browser Geolocation), leading with:
  1. **Feels-Like Temperature**
  2. **Actual / Regular Temperature**
  3. **Rain Outlook** (probability, expected rainfall, and active rain status)
- Display each underlying weather source in **collapsible submenus** beneath the aggregate estimate.
- Maintain a **lightweight 7-day rolling data store** (forecast snapshots + actual observations, no long-term archive, no user accounts) to run **forecast-vs-actuals regression** for future model weighting and tuning.
- Support **GitHub Pages + GitHub Actions** static deployment where feasible, with a shared-core **Home Server** deployment path when live scraping, tight schedules, or dynamic lookups are required.

## Repository Layout

```text
turtleheavyweather/
├── backend/     # Rust Cargo workspace: core domain, weather sources, store, collector CLI, optional Axum server
├── frontend/    # Leptos Rust web UI (WASM CSR for GitHub Pages, optional SSR/hydration for Home Server)
├── docs/        # Architecture, hosting feasibility, source catalog, API & regression specs, subagent plan
└── planning/    # Spec-driven Jira-esque epics, tickets, and live status board (BOARD.md)
```

## Where to Start

- **Hosting Feasibility (GitHub Pages vs Home Server):** [docs/hosting-feasibility.md](docs/hosting-feasibility.md)
- **System Architecture:** [docs/architecture.md](docs/architecture.md)
- **Australian Weather Sources:** [docs/weather-sources.md](docs/weather-sources.md)
- **API & Data Contracts:** [docs/api-spec.md](docs/api-spec.md)
- **Data Store & Regression Spec:** [docs/data-and-regression-spec.md](docs/data-and-regression-spec.md)
- **Subagent & Agent Factory Plan:** [docs/agent-factory-and-subagents.md](docs/agent-factory-and-subagents.md)
- **Work Board & Tickets:** [planning/BOARD.md](planning/BOARD.md)
