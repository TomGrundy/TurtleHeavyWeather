---
id: THW-002
epic: EPIC-02
title: Curate Australian suburb/postcode spatial index and representative station/hub mesh
area: backend
status: done
priority: P0
depends_on: []
---

## Context
Querying external APIs for all ~15,000+ Australian suburbs on a schedule would breach rate limits and bloat storage. Instead, we maintain:
1. A compact Australian suburb/postcode -> lat/lon lookup dataset.
2. A representative **Node Mesh** (~100–250 BOM AWS stations + regional hubs) used for scheduled weather collection, with every suburb/postcode and GPS coordinate mapped via nearest-neighbor Haversine distance.

## Spec & Design Notes
- Reference: [docs/architecture.md](../../docs/architecture.md), [docs/api-spec.md](../../docs/api-spec.md)
- Source open Australian postcode/suburb data (e.g., `schappim/australian-postcodes` or `matthewproctor/australianpostcodes`, MIT/open license — record attribution and license in `docs/`).
- Curate `data/au_nodes.json` (initial representative mesh covering all capital cities, major regional centers, and BOM AWS station IDs/product IDs).
- Build a repeatable generator script/binary that produces `locations_index.json` pre-linking each suburb/postcode to its nearest `node_id` and `distance_km`.

## Acceptance Criteria
- [ ] All 8 Australian states/territories (`NSW`, `VIC`, `QLD`, `SA`, `WA`, `TAS`, `NT`, `ACT`) are represented in `au_nodes.json`.
- [ ] Every suburb/postcode entry has valid coordinates within Australian bounding box (`lat ∈ [-45, -9]`, `lon ∈ [112, 155]`) and a valid nearest `node_id`.
- [ ] Compressed `locations_index.json` size is documented and remains lightweight (< 2 MB gzipped) for browser loading.

## Test Plan
- Unit tests verifying Haversine nearest-node lookup for sample postcodes (`2000` Sydney, `3000` Melbourne, `4000` Brisbane, `5000` Adelaide, `6000` Perth, `7000` Hobart, `0800` Darwin, `2600` Canberra).

## Execution Log
- *(pending)*
