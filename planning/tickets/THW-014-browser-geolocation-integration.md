---
id: THW-014
epic: EPIC-05
title: Browser navigator.geolocation integration and nearest-node Australian bounds handling
area: frontend
status: done
priority: P0
depends_on: [THW-013]
---

## Context
Users should be able to detect their current location via the browser's Geolocation API rather than manually typing a suburb or postcode.

## Spec & Design Notes
- Reference: [docs/api-spec.md](../../docs/api-spec.md)
- Implement `frontend/src/geolocation.rs` using `web_sys::Navigator::geolocation()`:
  - Triggered by **"Use My Location"** button (never prompts unexpectedly on initial page load without user intent, or configurable).
  - Handle loading spinner, permission-denied, timeout, and unavailable states clearly in UI.
  - Validate returned coordinates against Australian bounds (`lat ∈ [-45, -9]`, `lon ∈ [112, 155]`):
    - Inside Australia -> resolve nearest suburb & mesh `node_id` via `thw_core::find_nearest_node` (or `/api/v1/weather?lat=...&lon=...`).
    - Outside Australia -> display a friendly non-blocking notice and default/retain manual suburb picker.

## Acceptance Criteria
- [ ] Clicking "Use My Location" invokes `navigator.geolocation.getCurrentPosition` and updates the active location.
- [ ] Permission denial, unsupported browser, and out-of-Australia coordinates each render clear user-facing feedback.
- [ ] Zero coordinates are sent to third-party analytics or stored permanently.

## Test Plan
- Unit tests for coordinate-to-node resolution and Australian bounds checks; `wasm` compile check in CI.

## Execution Log
- *(pending)*
