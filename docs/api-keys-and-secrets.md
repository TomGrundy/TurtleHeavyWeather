# API Keys & GitHub Secrets Setup Guide

TurtleHeavyWeather is designed to work **out of the box with zero API keys** using free, keyless sources (Open-Meteo BOM ACCESS/ECMWF/GFS, BOM public JSON/FTP feeds, and MET Norway). Adding the optional free-tier API keys below increases forecast diversity and improves consensus accuracy.

---

## 1. Zero-Key Sources (No Signup Required)

| Source | Secret / Config Needed | Notes |
| :--- | :--- | :--- |
| **Open-Meteo** (BOM ACCESS-G, ECMWF, GFS) | None | Free for non-commercial use; multi-node batching enabled. |
| **BOM Public Station JSON & Anonymous FTP** | None (uses `THW_USER_AGENT` if set) | Official Australian observations & XML town forecasts. |
| **MET Norway (`api.met.no`)** | `THW_USER_AGENT` (optional override, has safe default) | Requires a descriptive `User-Agent` header (`AppName/Version (contact-url-or-email)`). |

---

## 2. Optional Free-Tier API Keys (Recommended)

| Provider | GitHub Secret Name | Where to Get Key | Free Tier Allowance | Credit Card Needed? |
| :--- | :--- | :--- | :--- | :--- |
| **WeatherAPI.com** | `THW_WEATHERAPI_KEY` | `https://www.weatherapi.com/signup.aspx` | 100,000 calls/month | No |
| **Pirate Weather** | `THW_PIRATEWEATHER_KEY` | `https://pirateweather.net/` (via API key portal) | 10,000 calls/month | No |
| **OpenWeatherMap** *(Standard 2.5)* | `THW_OWM_KEY` | `https://home.openweathermap.org/users/sign_up` | 60 calls/min (Current + 5d/3h) | No *(do not enable One Call 3.0 unless billing is configured)* |
| **Tomorrow.io** | `THW_TOMORROW_KEY` | `https://app.tomorrow.io/signup` | 500 calls/day, 25/hour *(collector caps to major capitals/hubs)* | No |
| **WillyWeather** *(Optional Paid AU)* | `THW_WILLYWEATHER_KEY` | `https://www.willyweather.com.au/info/api.html` | Paid per-request (fractions of a cent) | Yes (optional) |
| **Custom User-Agent** *(Recommended)* | `THW_USER_AGENT` | Any string identifying your repo, e.g. `TurtleHeavyWeather/0.1 (+https://github.com/<you>/turtleheavyweather)` | N/A | N/A |

> **60-Minute Cadence Note:** With a 60-minute sync (`24 runs/day`), stay safely inside free limits by keeping your mesh around ~100–120 nodes for `WeatherAPI.com` (~2,880/day? Wait: `100,000 / 30 = 3,333/day` -> ~138 calls/run max; collector automatically throttles or subsets keyed sources per budget!), ~15 major hubs for `Pirate Weather` (`10,000 / 30 = 333/day` -> ~13 calls/run), and ~8 capital hubs for `Tomorrow.io` (`500/day` -> ~20 calls/run max). The collector (`THW-006` / `THW-010`) enforces per-provider daily/hourly call budgets automatically so you never exhaust quotas.

---

## 3. Where to Put Keys in GitHub Secrets UI

1. Open your **public** GitHub repository in a browser.
2. Click **Settings** (top repository navigation bar).
3. In the left sidebar under **Security**, expand **Secrets and variables** and click **Actions**.
4. Under the **Repository secrets** tab, click **New repository secret**.
5. Add each secret using the **exact** Name from the table above (e.g., `THW_WEATHERAPI_KEY`) and paste your key into **Secret**, then click **Add secret**.

### Personal NUC (Home Server) Fallback Equivalent
When running on your personal NUC instead of GitHub Actions, put the exact same environment variable names into `/etc/turtleheavyweather/.env` (for `systemd`) or `.env` next to `compose.yaml` (for Docker Compose):

```dotenv
THW_USER_AGENT="TurtleHeavyWeather/0.1 (https://github.com/<your-user>/turtleheavyweather)"
THW_WEATHERAPI_KEY="..."
THW_PIRATEWEATHER_KEY="..."
THW_OWM_KEY="..."
THW_TOMORROW_KEY="..."
```
