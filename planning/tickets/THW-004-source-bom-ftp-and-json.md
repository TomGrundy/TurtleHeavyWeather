---
id: THW-004
epic: EPIC-02
title: BOM adapters (Public JSON observations, Anonymous FTP XML forecasts, optional web JSON)
area: backend
status: done
priority: P0
depends_on: [THW-003]
---

## Context
The Australian Bureau of Meteorology (BOM) is the authoritative baseline for Australian weather and provides our ground-truth station observations for regression.

## Spec & Design Notes
- Reference: [docs/weather-sources.md](../../docs/weather-sources.md)
- Implement in `thw-sources/src/bom/`:
  1. `BomObsJsonSource`: Fetches `http://www.bom.gov.au/fwo/{product_id}/{product_id}.{station_id}.json` with a descriptive `User-Agent`, parses 72h observations (`air_temp`, `apparent_t`, `rain_trace`, `rel_hum`, `wind_spd_kmh`), computes hourly rain delta from cumulative `rain_trace` (handling 9am local reset!).
  2. `BomFtpXmlSource`: Connects anonymously to `ftp.bom.gov.au/anon/gen/fwo/` (using `suppaftp` + `quick-xml`), parses AMOC XML town/district forecasts (`air_temperature_minimum`, `air_temperature_maximum`, `probability_of_precipitation`, `precipitation_range`, précis).
  3. `BomWebApiSource` (opt-in/circuit-broken): Queries `api.weather.bom.gov.au/v1/` by geohash for hourly/daily forecasts, degrading gracefully if schema changes.

## Acceptance Criteria
- [ ] 9am `rain_trace` reset edge case is handled accurately when computing hourly observed rainfall.
- [ ] All three BOM adapters map cleanly to `SourceReport` and handle HTTP/FTP timeouts without panicking.
- [ ] Recorded JSON and XML BOM fixtures live in `thw-sources/tests/fixtures/bom/`.

## Test Plan
- `wiremock` / fixture parser unit tests covering NSW, VIC, QLD JSON observations and AMOC XML forecast files, plus 9am rain-trace reset tests.

## Execution Log
- *(pending)*
