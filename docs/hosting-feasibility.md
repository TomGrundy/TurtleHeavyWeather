# Hosting Feasibility Analysis: GitHub Pages + Actions vs. Home Server

This document evaluates whether TurtleHeavyWeather can run **purely on GitHub Pages + GitHub Actions**, identifies the hard technical boundaries, and specifies the **Home Server** architecture where needed.

## 1. Verdict & Chosen Architecture

**Primary Target: Public GitHub Repo + GitHub Pages + GitHub Actions (60-minute cadence).**
**Fallback / Graduation Target: Personal NUC Home Server (`thw-server` + Docker/systemd + Caddy).**

Because this will be a **public repository**, GitHub Actions provides unlimited standard runner minutes and free GitHub Pages hosting, making a pure GitHub Pages + Actions architecture feasible with four understood constraints (pre-computed Australian mesh coverage, cron jitter, single-commit orphan state persistence, and cloud-IP scraping limits).

If any of those constraints become limiting later—or if you want sub-hourly updates, residential-IP scraping, or on-demand arbitrary suburb lookups—the **Personal NUC Home Server** fallback (`THW-012`) runs the exact same Rust core (`thw-core`, `thw-sources`, `thw-store`) and Leptos frontend with zero code rewrite.

---

## 2. Deep Dive: Can GitHub Pages + GitHub Actions Do Everything?

| Requirement | GitHub Pages + Actions Feasibility | Constraints & Mitigations |
| :--- | :--- | :--- |
| **Leptos Rust Web Frontend** | **Fully Supported** | Compile Leptos in **CSR (WASM)** mode via `trunk build --release`. Served natively as static HTML/JS/WASM on GitHub Pages. (Leptos SSR is *not* possible on GitHub Pages.) |
| **Regular Weather Collection Job** | **Supported with Caveats** | GitHub Actions `schedule` cron triggers are **best-effort**: delays of 15–90 minutes are common under peak load, runs can be skipped, and GitHub **auto-disables scheduled workflows after 60 days** without human commits/PRs. |
| **Repo & Data Store Limits (7-day rolling)** | **Supported with Workaround** | Never commit rolling snapshots to `main` (`.git` history will bloat toward GitHub's 1 GB soft / 5 GB hard limit). Instead: store the rolling 7-day SQLite file in a **force-pushed single-commit orphan branch (`data-state`)** or a **rolling GitHub Release asset**, and publish the built site + JSON shards via `actions/deploy-pages`. |
| **Suburb / Postcode Lookup + Geolocation** | **Supported via Mesh Mapping** | Browser holds a compact AU suburb/postcode index (~1–2 MB gzipped) and maps the selected suburb/postcode or `navigator.geolocation` lat/lon to the nearest pre-collected **station/region JSON shard** (`data/nodes/<id>.json`). |
| **API Keys & Rate Limits** | **Constrained** | Static sites cannot hold secret API keys in the browser. All keyed APIs must run inside the Actions collector across a **fixed mesh** (e.g., 80–200 key Australian hubs/stations) so you stay within free tiers (e.g., Tomorrow.io 500/day, PirateWeather 10k/mo). |
| **Web Scraping Weather Reports** | **Fragile from CI** | GitHub Actions runs on Azure cloud IPs, which commercial sites (Cloudflare-protected) and sometimes `www.bom.gov.au` may block or throttle, and passive FTP (`ftp.bom.gov.au`) needs firewall/TLS testing on runners. |
| **Actions Compute Minutes** | **Free on Public Repo** | Public repos get unlimited standard runner minutes. Private repos get **2,000 mins/month** (e.g., hourly 3-min jobs = ~2,160 mins/month -> would exceed private free tier). |

### Key Takeaway on Pure GitHub Pages
- Works well as a **public repo** using **no-key / generous-tier APIs** (Open-Meteo, MET Norway, BOM public feeds if reachable from runners) over a **~100-node Australian mesh**, updated every **1–3 hours**, deploying sharded static JSON + Leptos WASM.
- Fails if you require: **strict 10–15 min cadence**, **on-demand live scraping for any of 15,000 suburbs**, **private repo with frequent syncs**, or **scraping sites that block datacenter IPs**.

---

## 3. Home Server Hosting Plan (Fallback & Full-Featured Path)

If you prefer full reliability, strict scheduling, live on-demand fetching, and zero GitHub cron/IP constraints, host both backend and frontend on your home server.

### Recommended Home Server Stack

```text
[Internet / LAN]
      │
      ▼
[Reverse Proxy: Caddy or Nginx]  (Automatic HTTPS via Let's Encrypt or Tailscale)
      │
      ├── GET /                 ──► Leptos Frontend (static WASM bundle or SSR)
      └── GET /api/*            ──► thw-server (Axum on 127.0.0.1:8080)
                                        │
                                        ├── SQLite (WAL mode) ── 7-day rolling store + regression
                                        └── Background Tokio / systemd timer ── thw-collector
```

### Operational Design for Home Server
1. **Single-Binary or Two-Process Simplicity:**
   - Option A (Simplest): `thw-server` embeds/serves the compiled `frontend/dist` static files AND exposes `/api/v1/*` AND runs the background `thw-collector` loop on a `tokio::time::interval` (or triggered via `systemd` timer).
   - Option B (Docker Compose): Two containers (`thw-collector` + `thw-server`) sharing a bind-mounted SQLite volume (`/var/lib/turtleheavyweather/weather.db` in WAL mode) behind Caddy.
2. **Security & Networking (No Auth Needed):**
   - No user accounts or login required.
   - Expose read-only public `GET` endpoints only; bind admin/manual-trigger endpoints to `127.0.0.1` or omit them.
   - Access securely via **Cloudflare Tunnel**, **Tailscale Funnel / Tailnet** (ideal if personal/family use!), or **Caddy reverse proxy** with automatic TLS.
   - Add per-IP rate limiting in Caddy/tower-governor so external traffic cannot trigger upstream weather API exhaustion on cache-miss endpoints.
3. **Storage & Backups:**
   - Single SQLite file (`~20–100 MB` max with 7-day auto-pruning).
   - Daily `VACUUM` / `PRAGMA wal_checkpoint(TRUNCATE)` and automatic row deletion for records older than `now - 7 days`.
4. **CI/CD from GitHub:**
   - GitHub Actions runs tests/clippy on every PR, builds multi-arch release binaries or Docker images (`ghcr.io`), and your home server updates via Watchtower/systemd pull or SSH deploy.
