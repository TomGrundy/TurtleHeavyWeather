# Cloudflare Worker Hourly Cron Dispatcher

Reliably triggers `.github/workflows/sync-and-deploy.yml` via `workflow_dispatch` at `:17` past every hour UTC, avoiding GitHub Actions' best-effort `on.schedule` delays and dropped runs.

Runs well within Cloudflare Workers' free tier (100,000 requests/day; this uses 24 invocations/day).

## 1. Create a GitHub Fine-Grained Token

1. Go to **GitHub → Settings → Developer settings → Personal access tokens → Fine-grained tokens → Generate new token**.
2. **Repository access**: Only select repositories → `TomGrundy/TurtleHeavyWeather`.
3. **Permissions → Repository permissions**:
   - **Actions**: `Read and write` (required to trigger `workflow_dispatch`).
   - All other permissions can stay `No access`.
4. Copy the generated token.

## 2. Deploy the Worker

From this directory (`deploy/cloudflare-cron`):

```bash
cd deploy/cloudflare-cron

# Authenticate with your Cloudflare account
npx wrangler login

# Store the GitHub token as an encrypted Worker secret
npx wrangler secret put GITHUB_TOKEN

# Optional: set a secret for manual HTTP triggering via POST
npx wrangler secret put TRIGGER_SECRET

# Deploy the Worker and register the hourly cron trigger
npx wrangler deploy
```

## 3. Verify

- In the **Cloudflare Dashboard → Workers & Pages → `thw-cron-dispatcher` → Settings → Trigger Events**, confirm `17 * * * *` is listed.
- If you set `TRIGGER_SECRET`, you can trigger the deployed Worker manually at any time:
  ```bash
  curl -X POST https://thw-cron-dispatcher.<your-subdomain>.workers.dev \
    -H "Authorization: Bearer <your-trigger-secret>"
  ```
  Then confirm a `workflow_dispatch` run appears in GitHub Actions.
