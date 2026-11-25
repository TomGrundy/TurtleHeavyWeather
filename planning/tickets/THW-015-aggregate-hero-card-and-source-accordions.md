---
id: THW-015
epic: EPIC-05
title: Best-Estimate Hero Card (Feels-Like, Temp, Rain) and collapsible per-source accordions
area: frontend
status: done
priority: P0
depends_on: [THW-003, THW-013]
---

## Context
The UI must lead immediately with the aggregated Best Estimate—**Feels-Like Temperature**, **Regular Temperature**, and **Whether it is going to rain**—and place each underlying weather source inside collapsible submenus underneath.

## Spec & Design Notes
- Reference: [docs/architecture.md](../../docs/architecture.md), [docs/api-spec.md](../../docs/api-spec.md)
- Implement `frontend/src/components/`:
  1. `best_estimate_card.rs`:
     - **Primary metric #1:** Feels-Like Temperature (`°C`) large hero readout.
     - **Primary metric #2:** Regular Temperature (`°C`) + source min/max spread.
     - **Primary metric #3:** Rain outlook badge & explanation (`Raining Now` / `Rain Likely` / `Possible Showers` / `Dry`, consensus probability `%`, expected `mm`).
     - Compact 24h hourly strip of Best Estimates + data freshness timestamp & source count badge.
  2. `source_accordions.rs`:
     - Below the hero card, render a list of **collapsible submenus** (`<details>` / accessible Leptos accordion) — **one per weather source** (collapsed by default, with a summary header showing source name, Feels-Like, Temp, Rain %, and freshness).
     - Expanding a submenu reveals that source's detailed current metrics, weight in the consensus, derived-feels-like badge (if computed by Steadman fallback), and hourly forecast rows.

## Acceptance Criteria
- [ ] Hero card visually leads with Feels-Like Temperature, Regular Temperature, and Rain Verdict above the fold.
- [ ] Every source returned in `NodeWeatherBundle.sources` renders in its own collapsible submenu below the hero card.
- [ ] Works gracefully when only a subset of sources is available or a source omits rain probability.

## Test Plan
- Component rendering tests against recorded `NodeWeatherBundle` fixtures verifying all required fields and accordion expand/collapse DOM state.

## Execution Log
- *(pending)*
