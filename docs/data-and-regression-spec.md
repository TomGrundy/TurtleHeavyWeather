# Lightweight 7-Day Data Store & Regression Spec

## 1. Design Goals

- **No long-term history:** Retain only a rolling **7-day lookback** of forecast snapshots and observations, plus active upcoming forecasts (up to 7–10 days ahead).
- **Zero-ops footprint:** Single embedded **SQLite** database (`weather.db`, WAL mode) keeping total disk footprint tiny (~10–80 MB) with automated pruning at the end of every collector run.
- **Forecast-vs-Actual Regression:** Enable quantitative comparison between:
  1. Each individual source's prior predictions (at 1h, 6h, 24h, 48h lead times) vs. observed ground truth.
  2. The TurtleHeavyWeather **Best Estimate** aggregate vs. observed ground truth.
  So future iterations can optimize source weights and bias corrections empirically.

---

## 2. Logical SQLite Schema (`thw-store`)

```sql
-- Representative Australian mesh nodes (BOM AWS stations + regional hubs)
CREATE TABLE nodes (
    node_id TEXT PRIMARY KEY,          -- e.g., "bom-95936" or "hub-sydney-cbd"
    name TEXT NOT NULL,
    state TEXT NOT NULL,               -- NSW, VIC, QLD, SA, WA, TAS, NT, ACT
    lat REAL NOT NULL,
    lon REAL NOT NULL,
    bom_station_id TEXT,               -- Optional BOM AWS station ID for ground-truth obs
    bom_product_id TEXT                -- e.g., "IDV60901"
);

-- Individual source forecast snapshots (retained 7 days past valid_at)
CREATE TABLE forecast_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id TEXT NOT NULL REFERENCES nodes(node_id),
    source_id TEXT NOT NULL,           -- e.g., "open_meteo_bom_access", "met_no", "bom_ftp"
    issued_at TEXT NOT NULL,           -- RFC3339 UTC timestamp when fetched/issued
    valid_at TEXT NOT NULL,            -- RFC3339 UTC timestamp being predicted
    lead_hours INTEGER NOT NULL,       -- round((valid_at - issued_at) in hours)
    temp_c REAL,
    feels_like_c REAL,
    feels_like_is_derived INTEGER NOT NULL DEFAULT 0,
    rain_prob_pct REAL,                -- 0..100
    rain_mm REAL,                      -- expected mm in interval
    wind_kmh REAL,
    humidity_pct REAL,
    summary TEXT,
    UNIQUE(node_id, source_id, issued_at, valid_at)
);

-- Aggregate "Best Estimate" snapshots (retained 7 days past valid_at)
CREATE TABLE aggregate_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id TEXT NOT NULL REFERENCES nodes(node_id),
    issued_at TEXT NOT NULL,
    valid_at TEXT NOT NULL,
    lead_hours INTEGER NOT NULL,
    feels_like_c REAL NOT NULL,
    temp_c REAL NOT NULL,
    rain_verdict TEXT NOT NULL,        -- "dry" | "possible" | "likely" | "raining"
    rain_prob_pct REAL NOT NULL,
    rain_mm_expected REAL NOT NULL,
    source_count INTEGER NOT NULL,
    weights_version TEXT NOT NULL,     -- e.g., "v1-static" or "v2-tuned"
    UNIQUE(node_id, issued_at, valid_at)
);

-- Observed ground-truth weather (from BOM AWS JSON feeds, retained 7 days)
CREATE TABLE observations (
    node_id TEXT NOT NULL REFERENCES nodes(node_id),
    observed_at TEXT NOT NULL,         -- RFC3339 UTC
    source_id TEXT NOT NULL,           -- typically "bom_obs_json"
    temp_c REAL,
    feels_like_c REAL,
    rain_mm_1h REAL,                   -- derived delta from rain_trace since 9am
    rained_bool INTEGER,               -- 1 if rain_mm_1h >= 0.2mm
    wind_kmh REAL,
    humidity_pct REAL,
    PRIMARY KEY (node_id, observed_at, source_id)
);

-- Materialized 7-day rolling regression metrics per (source_or_aggregate, lead_bucket)
CREATE TABLE regression_metrics (
    computed_at TEXT NOT NULL,
    window_days INTEGER NOT NULL DEFAULT 7,
    subject_id TEXT NOT NULL,          -- source_id or "__aggregate__"
    lead_bucket TEXT NOT NULL,         -- "0-6h" | "6-24h" | "24-72h"
    sample_count INTEGER NOT NULL,
    temp_mae_c REAL,                   -- Mean Absolute Error (°C)
    temp_rmse_c REAL,                  -- Root Mean Squared Error (°C)
    temp_bias_c REAL,                  -- Mean signed error (forecast - actual)
    feels_like_mae_c REAL,
    feels_like_rmse_c REAL,
    rain_brier_score REAL,             -- Mean (p_forecast/100 - actual_rained)^2
    rain_hit_rate REAL,                -- Accuracy at 50% threshold
    PRIMARY KEY (subject_id, lead_bucket)
);
```

---

## 3. Retention & Pruning Policy

Run after every collector sync:
1. Delete `forecast_snapshots` where `valid_at < now - 7 days`.
2. Delete `aggregate_snapshots` where `valid_at < now - 7 days`.
3. Delete `observations` where `observed_at < now - 7 days`.
4. Recompute `regression_metrics` over the remaining 7-day window.

---

## 4. Ground Truth Selection & Regression Methodology

- **Ground Truth Hierarchy ("Most Accurate Historical Actuals"):**
  - **Primary (Tier 1):** Direct physical **BOM AWS Station Observations (`bom_obs_json`)** for the node (or nearest BOM AWS station within 25 km), because physical thermometer and rain-gauge measurements represent actual ground truth rather than model output.
  - **Secondary (Tier 2, Fallback / Validation):** **Open-Meteo Historical / ERA5 Reanalysis** or multi-source observation consensus (`weatherapi_current`, `owm_current`) when a BOM AWS station is offline or out of range, subject to physical sanity checks (e.g., rejecting stuck sensors or impossible jumps).
  - **Auditability:** Each `observations` row records its `source_id` and `quality_rank` so regression reports clearly show what ground-truth source evaluated each prediction.
- **Ground Truth Pairing:** Match each `forecast_snapshots` / `aggregate_snapshots` row (`valid_at`) to the highest-ranked `observations` row within `±30 minutes` for the same `node_id`.
- **Metrics Computed:**
  - **Temperature & Feels-Like:** `MAE`, `RMSE`, and signed `Bias` (`forecast - actual`) across lead-time buckets (`0–6h`, `6–24h`, `24–72h`).
  - **Rain Accuracy:** **Brier Score** `(p - y)^2` (where `p ∈ [0, 1]` and `y ∈ {0, 1}` for `rain_mm_1h >= 0.2mm`) + Hit Rate + False Alarm Rate.
- **How This Feeds Future Optimization:**
  - Exporting `regression_metrics` (and optional CSV/JSON summary for local notebook/script analysis) lets you inspect which sources consistently run warm/cold or over-predict rain in Sydney vs Melbourne vs Darwin, and tune `weights.toml` (inverse-RMSE weighting + per-source bias offset).
