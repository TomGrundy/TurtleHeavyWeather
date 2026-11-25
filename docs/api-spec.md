# API & Static Data Contract (`docs/api-spec.md`)

TurtleHeavyWeather defines one unified JSON contract consumed by the Leptos frontend, served either as **pre-generated static JSON shards** (GitHub Pages) or **live REST responses** (Home Server `thw-server`).

---

## 1. Common JSON Schemas

### 1.1 Suburb / Postcode Index (`locations_index.json` / `GET /api/v1/locations?q=...`)

For GitHub Pages, shipped as a compact static JSON/JSON.gz index mapping Australian suburbs & postcodes to their nearest mesh `node_id`:

```json
[
  {
    "suburb": "Surry Hills",
    "postcode": "2010",
    "state": "NSW",
    "lat": -33.8861,
    "lon": 151.2111,
    "node_id": "bom-066062",
    "distance_km": 2.4
  }
]
```

### 1.2 Node Weather Bundle (`nodes/{node_id}.json` / `GET /api/v1/weather?lat=...&lon=...` or `?node_id=...`)

```json
{
  "generated_at": "2026-11-25T00:00:00Z",
  "node": {
    "node_id": "bom-066062",
    "name": "Sydney (Observatory Hill)",
    "state": "NSW",
    "lat": -33.8593,
    "lon": 151.2048
  },
  "current_best_estimate": {
    "valid_at": "2026-11-25T00:00:00Z",
    "feels_like_c": 23.4,
    "temp_c": 24.1,
    "rain": {
      "verdict": "possible",
      "label": "Possible light showers",
      "probability_pct": 35,
      "expected_mm": 0.6,
      "is_raining_now": false
    },
    "confidence": "high",
    "spread": {
      "feels_like_min_c": 22.1,
      "feels_like_max_c": 24.8,
      "temp_min_c": 23.0,
      "temp_max_c": 25.2
    },
    "source_count": 5
  },
  "hourly_best_estimate": [
    {
      "valid_at": "2026-11-25T01:00:00Z",
      "feels_like_c": 24.0,
      "temp_c": 24.7,
      "rain_prob_pct": 30,
      "rain_mm_expected": 0.2
    }
  ],
  "sources": [
    {
      "source_id": "bom_obs_json",
      "display_name": "BOM Station Observation",
      "kind": "observation",
      "fetched_at": "2026-11-25T00:00:00Z",
      "weight": 0.35,
      "current": {
        "feels_like_c": 23.2,
        "feels_like_is_derived": false,
        "temp_c": 24.0,
        "rain_prob_pct": null,
        "rain_mm": 0.0,
        "wind_kmh": 18.0,
        "humidity_pct": 62.0,
        "summary": "Partly cloudy"
      },
      "hourly": []
    }
  ]
}
```

### 1.3 Regression Summary (`regression/summary.json` / `GET /api/v1/regression`)

```json
{
  "computed_at": "2026-11-25T00:05:00Z",
  "window_days": 7,
  "metrics": [
    {
      "subject_id": "__aggregate__",
      "display_name": "TurtleHeavyWeather Best Estimate",
      "lead_bucket": "0-6h",
      "sample_count": 420,
      "temp_mae_c": 0.92,
      "temp_rmse_c": 1.18,
      "temp_bias_c": -0.11,
      "feels_like_mae_c": 1.15,
      "feels_like_rmse_c": 1.44,
      "rain_brier_score": 0.112,
      "rain_hit_rate": 0.86
    }
  ]
}
```

---

## 2. Home Server REST API (`thw-server`, Axum)

| Method & Path | Query Params | Description |
| :--- | :--- | :--- |
| `GET /healthz` | — | Liveness/readiness check + last collector sync timestamp |
| `GET /api/v1/locations` | `q` (suburb or postcode prefix), `limit` (default 15) | Fast suburb/postcode autocomplete search |
| `GET /api/v1/weather` | `node_id` OR (`lat`, `lon`) OR `postcode` | Returns `NodeWeatherBundle` (on-demand cache fetch if stale on Home Server) |
| `GET /api/v1/regression` | `window_days` (default 7), `node_id` (optional) | Returns `RegressionSummary` |

---

## 3. Browser Geolocation Flow

1. User clicks **"Use My Location"** in the Leptos UI.
2. Frontend invokes `web_sys::Navigator::geolocation()` -> `get_current_position`.
3. Browser returns `{ latitude, longitude }` (validated to Australian bounding box `lat ∈ [-45, -9]`, `lon ∈ [112, 155]`; if outside Australia, show clear banner explaining coverage is Australia-only).
4. **GitHub Pages mode:** Frontend finds nearest `node_id` in client-side index and fetches `data/nodes/{node_id}.json`.
5. **Home Server mode:** Frontend calls `GET /api/v1/weather?lat=...&lon=...`.
