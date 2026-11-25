# Frontend (`frontend/`)

Planned **Leptos** (Rust -> WebAssembly) web application for TurtleHeavyWeather.

> **Status:** Planning phase. See [planning/BOARD.md](../planning/BOARD.md) and [docs/architecture.md](../docs/architecture.md).

## Key UI & Technical Features

- **Location Lookup & Geolocation:** Search by Australian suburb or 4-digit postcode (client-side prefix/fuzzy search over a compact compressed/JSON index) or click **"Use My Location"** using the browser `navigator.geolocation` API (`THW-013`, `THW-014`).
- **Best-Estimate Hero Card:** Leads prominently with:
  1. **Feels-Like Temperature** (`°C`)
  2. **Temperature** (`°C`)
  3. **Will It Rain?** (Yes/No/Chance summary, probability %, expected `mm`, and current rain status)
  plus confidence / model spread indicator (`THW-015`).
- **Collapsible Source Breakdown:** Expandable `<details>` / accordion panels below the hero card displaying each individual source's forecast, observation timestamp, and source link (`THW-015`).
- **Regression Diagnostics View:** Lightweight view showing 7-day forecast-vs-actuals MAE/RMSE/rain-Brier score per source to guide weighting optimizations (`THW-016`).
- **Dual Data-Client Transport:**
  - `StaticJsonClient`: Fetches pre-built static JSON shards on **GitHub Pages** (`trunk build --release`).
  - `HttpApiClient`: Fetches live/cached endpoints from `thw-server` on **Home Server**.
- **No Authentication:** Completely public, zero login or user-account state.
