---
id: THW-006
epic: EPIC-02
title: Optional free-tier keyed APIs and robots/ToS-compliant Australian weather scrapers
area: backend
status: done
priority: P1
depends_on: [THW-003]
---

## Context
To maximize Australian data diversity, we want optional adapters for free-tier keyed APIs and an audited, compliant HTML scraping framework for public Australian weather report pages.

## Spec & Design Notes
- Reference: [docs/weather-sources.md](../../docs/weather-sources.md)
- Keyed API adapters (activated automatically only when env var is present; gracefully skipped otherwise):
  - `WeatherApiSource` (`THW_WEATHERAPI_KEY`)
  - `PirateWeatherSource` (`THW_PIRATEWEATHER_KEY`)
  - `OpenWeatherMapSource` (`THW_OWM_KEY`)
  - `TomorrowIoSource` (`THW_TOMORROW_KEY`, rate-capped to capital/hub subset)
- Web Scraper Audit & Implementation (`scraper` crate):
  - Document `robots.txt` and Terms-of-Service findings in `docs/weather-sources.md` before enabling any HTML target.
  - Implement `BomHtmlForecastScraper` (parsing public server-rendered BOM place pages as an additional/fallback source).
  - Add any other verified `robots.txt`-permitting static HTML AU weather report pages discovered during audit.

## Acceptance Criteria
- [ ] Missing API keys cleanly disable only that adapter with an info log (no errors).
- [ ] Rate budget guard prevents exceeding daily/hourly free-tier limits (e.g., Tomorrow.io 25 req/hr).
- [ ] Scraper audit results are documented in `docs/weather-sources.md`; only compliant HTML targets are enabled by default.

## Test Plan
- `wiremock` tests for each keyed API response schema and HTML fixture parsing tests for scrapers.

## Execution Log
- *(pending)*
