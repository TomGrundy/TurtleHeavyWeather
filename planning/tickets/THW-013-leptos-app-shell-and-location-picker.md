---
id: THW-013
epic: EPIC-05
title: Leptos app shell, WeatherDataClient transport abstraction, and suburb/postcode search
area: frontend
status: done
priority: P0
depends_on: [THW-001, THW-002]
---

## Context
The Leptos WASM frontend must work seamlessly against either GitHub Pages static JSON shards or the Home Server REST API, and let users quickly pick any Australian suburb or postcode without an account.

## Spec & Design Notes
- Reference: [docs/api-spec.md](../../docs/api-spec.md), [frontend/README.md](../../frontend/README.md)
- Implement `frontend/src/`:
  - `client.rs`: `WeatherDataClient` trait/enum supporting `StaticJson` (`/data/...`) and `HttpApi` (`/api/v1/...`), selected via compile-time feature or `Trunk`/meta config.
  - `components/location_search.rs`: Instant autocomplete search input matching Australian suburb names or 4-digit postcodes (with keyboard navigation & ARIA combobox roles).
  - Persist last selected location in browser `localStorage` (no backend account required).
  - Responsive mobile-first layout + clean dark/light weather styling.

## Acceptance Criteria
- [ ] Compiles cleanly with `trunk build` (`wasm32-unknown-unknown`).
- [ ] Typing `"2000"` or `"Surry Hills"` filters Australian locations instantaneously and triggers weather load.
- [ ] Works offline/locally against mock fixture JSON files in `frontend/static/mock/`.

## Test Plan
- Pure Rust unit tests for suburb/postcode ranking & matching logic + `wasm-bindgen-test` / headless smoke check in CI.

## Execution Log
- *(pending)*
