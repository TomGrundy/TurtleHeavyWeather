# Custom Domain (`turtleheavy.com`) & iOS App Bookmark Setup

## 1. Namecheap DNS Configuration (`turtleheavy.com`)

1. Sign in to **Namecheap** → **Domain List** → click **Manage** next to `turtleheavy.com`.
2. Open the **Advanced DNS** tab.
3. Remove any conflicting default `A`, `CNAME`, or `URL Redirect` records for `@` and `www`.
4. Add these **5 DNS records** (TTL: `Automatic`):

| Type | Host | Value |
| :--- | :--- | :--- |
| `A Record` | `@` | `185.199.108.153` |
| `A Record` | `@` | `185.199.109.153` |
| `A Record` | `@` | `185.199.110.153` |
| `A Record` | `@` | `185.199.111.153` |
| `CNAME Record` | `www` | `tomgrundy.github.io.` |

---

## 2. GitHub Repository Pages Configuration

1. Open `https://github.com/TomGrundy/TurtleHeavyWeather` → **Settings → Pages**.
2. Under **Custom domain**, enter `turtleheavy.com` and click **Save**.
3. Wait for DNS check to pass (usually 5–30 minutes after updating Namecheap), then check **Enforce HTTPS**.
4. *(Already configured in repo)*: `frontend/CNAME` contains `turtleheavy.com` and `.github/workflows/sync-and-deploy.yml` builds Trunk with `--public-url "/"`.

---

## 3. iOS "Add to Home Screen" & Logo Assets

- `frontend/static/assets/` contains `logo-color.png`, `logo-white.png` (used in the top-left dark header), `logo-black.png`, `favicon.ico`, `favicon-16x16.png`, `favicon-32x32.png`, `apple-touch-icon.png` (`180x180`), `icon-192.png`, and `icon-512.png`.
- On iPhone/iPad Safari: open `https://turtleheavy.com`, tap the **Share** button, and tap **Add to Home Screen** — it launches full-screen (`standalone`) with the `TurtleHeavy` icon.
