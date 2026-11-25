#!/usr/bin/env python3
"""Build compact Australian suburb/postcode index mapped to nearest mesh nodes.

Reads:
  - data/au_nodes.json
  - /tmp/au_postcodes.json (or path passed as argv[1])
Outputs:
  - data/locations_index.json
  - frontend/static/data/locations_index.json
"""

from __future__ import annotations

import gzip
import json
import math
import pathlib
import sys


AU_LAT_MIN, AU_LAT_MAX = -45.0, -9.0
AU_LON_MIN, AU_LON_MAX = 112.0, 155.0
VALID_STATES = {"NSW", "VIC", "QLD", "SA", "WA", "TAS", "NT", "ACT"}


def haversine_km(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    r = 6371.0
    phi1, phi2 = math.radians(lat1), math.radians(lat2)
    dphi = math.radians(lat2 - lat1)
    dlambda = math.radians(lon2 - lon1)
    a = (
        math.sin(dphi / 2.0) ** 2
        + math.cos(phi1) * math.cos(phi2) * math.sin(dlambda / 2.0) ** 2
    )
    return 2.0 * r * math.atan2(math.sqrt(a), math.sqrt(1.0 - a))


def title_case_suburb(name: str) -> str:
    return " ".join(part.capitalize() for part in name.strip().split())


def main() -> None:
    repo_root = pathlib.Path(__file__).resolve().parent.parent
    raw_path = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else pathlib.Path("/tmp/au_postcodes.json")
    nodes_path = repo_root / "data" / "au_nodes.json"
    out_path = repo_root / "data" / "locations_index.json"
    frontend_out_path = repo_root / "frontend" / "static" / "data" / "locations_index.json"

    nodes = json.loads(nodes_path.read_text(encoding="utf-8"))
    raw_entries = json.loads(raw_path.read_text(encoding="utf-8"))

    seen: set[tuple[str, str, str]] = set()
    index: list[dict[str, object]] = []

    for row in raw_entries:
        postcode = str(row.get("postcode", "")).strip().zfill(4)
        locality = title_case_suburb(str(row.get("locality", "")))
        state = str(row.get("state", "")).strip().upper()
        if not postcode or not locality or state not in VALID_STATES:
            continue

        lat_val = row.get("Lat_precise") or row.get("lat")
        lon_val = row.get("Long_precise") or row.get("long")
        try:
            lat = float(lat_val)
            lon = float(lon_val)
        except (TypeError, ValueError):
            continue

        if not (AU_LAT_MIN <= lat <= AU_LAT_MAX and AU_LON_MIN <= lon <= AU_LON_MAX):
            continue

        key = (postcode, locality.lower(), state)
        if key in seen:
            continue
        seen.add(key)

        best_node = None
        best_dist = float("inf")
        for node in nodes:
            d = haversine_km(lat, lon, float(node["lat"]), float(node["lon"]))
            if d < best_dist:
                best_dist = d
                best_node = node

        if best_node is None:
            continue

        index.append(
            {
                "suburb": locality,
                "postcode": postcode,
                "state": state,
                "lat": round(lat, 4),
                "lon": round(lon, 4),
                "node_id": best_node["node_id"],
                "distance_km": round(best_dist, 1),
            }
        )

    index.sort(key=lambda x: (str(x["postcode"]), str(x["suburb"])))
    payload = json.dumps(index, separators=(",", ":"), ensure_ascii=False)
    out_path.write_text(payload, encoding="utf-8")
    frontend_out_path.parent.mkdir(parents=True, exist_ok=True)
    frontend_out_path.write_text(payload, encoding="utf-8")

    gz_bytes = len(gzip.compress(payload.encode("utf-8")))
    print(
        f"Wrote {len(index)} Australian locations "
        f"({len(payload)} bytes raw, {gz_bytes} bytes gzipped) to {out_path}"
    )


if __name__ == "__main__":
    main()
