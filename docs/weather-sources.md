# Australian Weather Sources Catalog

This document inventories accessible weather APIs, official government feeds, and web-scrapable reports for Australia, along with authentication, rate limits, data fields, and compliance notes.

> **Implementation Rule:** Every source adapter in `thw-sources` must be feature-flagged or config-enabled, handle timeouts/failures gracefully, and be tested with recorded mock fixtures in CI.

---

## Tier 1: Free, Keyless, High-Reliability Sources (Core Baseline)

### 1. Open-Meteo Forecast API (including BOM ACCESS-G)
- **Endpoint:** `https://api.open-meteo.com/v1/forecast`
- **Auth:** None (free for non-commercial use; API key required only for commercial tier).
- **Australian Coverage:** Excellent. Exposes **Australian Bureau of Meteorology `bom_access_global` (ACCESS-G)** model directly alongside **ECMWF IFS**, **NOAA GFS**, and **DWD ICON**, allowing us to treat different meteorological models as distinct comparable inputs!
- **Key Features:** Supports multi-coordinate batching (`latitude=...&longitude=...`), hourly `temperature_2m`, `apparent_temperature` (feels-like), `precipitation_probability`, `precipitation`, `rain`, `wind_speed_10m`, `relative_humidity_2m`.
- **Limits:** Fair-use (~10,000 calls/day soft threshold; batching 50–100 coordinates per call keeps usage under ~100 calls/day).

### 2. Bureau of Meteorology (BOM) — Public JSON Station Observations
- **Endpoint Pattern:** `http://www.bom.gov.au/fwo/{PRODUCT_ID}/{PRODUCT_ID}.{STATION_ID}.json`
  - State prefixes: `IDN60901` (NSW/ACT), `IDV60901` (VIC), `IDQ60901` (QLD), `IDS60901` (SA), `IDW60901` (WA), `IDT60901` (TAS), `IDD60901` (NT).
- **Auth:** None (public non-commercial feeds; requires descriptive browser-like/app `User-Agent` header).
- **Data:** Last 72 hours of 10–30 minute **actual ground-truth observations**: `air_temp`, `apparent_t` (BOM Steadman feels-like), `rain_trace` (since 9am), `rel_hum`, `wind_spd_kmh`, `press`.
- **Role in TurtleHeavyWeather:** Critical as **both** current conditions display AND the **ground-truth baseline** for the 7-day forecast-vs-actuals regression engine!

### 3. Bureau of Meteorology (BOM) — Anonymous FTP XML Forecasts
- **Endpoint:** `ftp://ftp.bom.gov.au/anon/gen/fwo/` (XML AMOC precinct/town forecasts: `IDN11060.xml`, `IDV10753.xml`, etc.)
- **Auth:** Anonymous FTP (`anonymous` / email).
- **Data:** Official BOM 7-day town/district forecasts (`air_temperature_minimum`, `air_temperature_maximum`, `probability_of_precipitation`, `precipitation_range`, précis text, UV/warnings).
- **Caveats:** FTP protocol requires an async Rust FTP client (`suppaftp`) and testing passive-mode connectivity in GitHub Actions / Docker.

### 4. MET Norway (`api.met.no`) Locationforecast 2.0
- **Endpoint:** `https://api.met.no/weatherapi/locationforecast/2.0/compact?lat={lat}&lon={lon}`
- **Auth:** No API key; **requires** unique `User-Agent` header identifying the app and contact URL/email (requests without it get `403`).
- **License:** CC BY 4.0 (commercial and non-commercial permitted with attribution).
- **Data:** Hourly & 6-hourly forecasts (`air_temperature`, `precipitation_amount`, `probability_of_precipitation` where available, `wind_speed`, `relative_humidity`). Powered by ECMWF/global models.
- **Limits:** Respect `Expires` / `If-Modified-Since` headers; keep request rates polite (e.g., <20 req/sec, throttle sequentially).

---

## Tier 2: Unofficial / Semi-Public Australian Endpoints

### 5. BOM Modern Web/App JSON API (`api.weather.bom.gov.au`)
- **Endpoints:** `https://api.weather.bom.gov.au/v1/locations/{geohash}/forecasts/hourly` (and `/daily`, `/observations`).
- **Auth:** None.
- **Data:** Hyper-local hourly and daily BOM forecasts (feels-like, temp, rain chance %, rain range mm).
- **Risk:** Undocumented internal endpoint; subject to schema changes or blocking without notice. Must be isolated in its own adapter with circuit-breaker/fallback so breaks don't degrade the app.

---

## Tier 3: Free-Tier API-Key Sources (Optional Env-Var Gated)

Enabled automatically when corresponding environment variable / GitHub Secret is set:

| Source | Free Allowance | Key Env Var | Notes |
| :--- | :--- | :--- | :--- |
| **WeatherAPI.com** | 100,000 calls/month | `THW_WEATHERAPI_KEY` | Supports AU postcodes & lat/lon; 14-day + hourly + `feelslike_c` + `chance_of_rain`. |
| **Pirate Weather** | 10,000 calls/month | `THW_PIRATEWEATHER_KEY` | NOAA GFS/HRRR + ERA5; Dark Sky JSON schema; personal/non-commercial focus. |
| **OpenWeatherMap** | 60 calls/min (2.5 Standard) | `THW_OWM_KEY` | Current + 5-day/3-hr forecast without credit card (One Call 3.0 requires card). |
| **Tomorrow.io** | 500 calls/day (25/hr) | `THW_TOMORROW_KEY` | Strict hourly rate limit; best reserved for capital cities / major hubs only. |
| **WillyWeather** | Paid (fractions of a cent/req) | `THW_WILLYWEATHER_KEY` | Optional paid plugin for hyper-local AU suburb/coast/tide/UV enrichment. |

---

## Tier 4: Web-Scrapable Australian Weather Reports (Discovery & Compliance Required)

Before enabling any HTML scraper in production, `THW-006` requires verifying `robots.txt`, Terms of Service, and non-JS HTML availability:

1. **BOM Public HTML Forecast & Observation Summary Pages** (`www.bom.gov.au/places/...`, `www.bom.gov.au/{state}/observations/...`):
   - Plain server-rendered HTML tables; easy to parse with `scraper` crate as fallback if FTP is blocked.
2. **Elders Weather / Weatherzone / Seabreeze Public Pages**:
   - **Caution:** Many commercial AU weather websites use Cloudflare bot protection, dynamic JS hydration, or ToS clauses prohibiting automated scraping. Ticket `THW-006` includes an explicit **compliance & technical audit gate**: only static, `robots.txt`-permitting, ToS-compliant HTML endpoints may be enabled by default; others remain documented or opt-in for local research.
