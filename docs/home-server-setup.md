# Personal NUC (Home Server) Deployment Runbook

Use this guide if you later migrate TurtleHeavyWeather off GitHub Pages onto your personal NUC.

## Option A: Docker Compose + Caddy (Recommended)

1. Copy `.env` next to `deploy/home-server/compose.yaml` using the exact variable names from [api-keys-and-secrets.md](api-keys-and-secrets.md).
2. Update `deploy/home-server/Caddyfile` with your domain (or use Tailscale Serve `tailscale serve 8080` if keeping it private to your tailnet).
3. Start the stack:
   ```bash
   cd deploy/home-server
   docker compose up -d --build
   ```

## Option B: Bare-Metal Linux `systemd`

1. Build `thw-server` and `frontend/dist`:
   ```bash
   cargo build --release -p thw-server
   cd frontend && THW_TRANSPORT=api trunk build --release
   ```
2. Install binary to `/usr/local/bin/thw-server`, static files/data to `/opt/turtleheavyweather/`, and `.env` to `/etc/turtleheavyweather/.env`.
3. Enable and start `deploy/home-server/turtleheavyweather.service`:
   ```bash
   sudo cp deploy/home-server/turtleheavyweather.service /etc/systemd/system/
   sudo systemctl enable --now turtleheavyweather
   ```
