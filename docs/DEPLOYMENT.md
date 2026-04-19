# Deployment & Operations

This document covers everything the localhost-first Quick Start in the README does **not**: public hostnames, TLS, Cloudflare Tunnel, reverse proxies, upgrades, rollbacks.

If you just want to try NetSentinel on your laptop or homelab, stop here and go back to [`README.md`](../README.md) → Quick Start.

---

## 1. Same-origin vs split-origin

Out of the box the dashboard and the API share one origin — whatever hostname serves port `3000`. CORS is effectively unused, `SameSite=Strict` cookies Just Work.

You only need to think about origins when an operator puts the dashboard and the API on **different hostnames**, typically behind a reverse proxy:

```
browser ─┬─→ https://dashboard.example.com    (static bundle)
         └─→ https://api.example.com          (Axum API + SSE)
```

In that case you must:

1. Build the web bundle with the API hostname baked in:
   ```bash
   NEXT_PUBLIC_API_URL=https://api.example.com docker compose up -d --build server
   ```
2. Add **both** hostnames to `ALLOWED_ORIGINS` in `.env`:
   ```
   ALLOWED_ORIGINS=https://dashboard.example.com,https://api.example.com
   ```
3. Make sure the reverse proxy forwards SSE correctly (`proxy_buffering off` for nginx, or native WebSocket/SSE handling for Caddy / Traefik).

---

## 2. Cloudflare Tunnel (optional)

NetSentinel is designed to run behind Cloudflare Zero Trust, with the server **pulling** from agents through the tunnel. Nothing in `docker-compose.yml` assumes this — you add it with an override.

### 2.1 Base: run the tunnel as a sibling service

Create `docker-compose.tunnel.yml`:

```yaml
services:
  tunnel:
    image: cloudflare/cloudflared:latest
    restart: unless-stopped
    command: tunnel --no-autoupdate run
    environment:
      - TUNNEL_TOKEN=${CLOUDFLARE_TUNNEL_TOKEN}
    networks:
      - backend-internal

networks:
  backend-internal:
    external: false
```

Add `CLOUDFLARE_TUNNEL_TOKEN=…` to `.env` and bring both compose files up:

```bash
docker compose -f docker-compose.yml -f docker-compose.tunnel.yml up -d
```

### 2.2 Configure the tunnel

In the Cloudflare Zero Trust dashboard, route your public hostname to the internal service. Example:

| Public hostname | Service URL inside the stack |
|---|---|
| `https://dashboard.example.com` | `http://server:3000` |

Both the UI and API are on the same origin, so a single hostname is all you need.

### 2.3 Scrape agents over the tunnel

Agents register their own public hostname (e.g. `agent1.example.com`) in Cloudflare and the server reaches them as `http://agent1.example.com/metrics`. The `host_key` in `/api/hosts` should then be `agent1.example.com:443` (port is required in the key format).

---

## 3. Upgrading

NetSentinel migrates the DB schema forward on every server start via `sqlx::migrate!()`. To upgrade:

```bash
cd netsentinel
git pull                      # get the new release
docker compose up -d --build  # rebuild + restart
./scripts/smoke-test.sh       # verify the upgrade
```

There is no downtime-safe rolling upgrade yet: `docker compose up` recreates the server container atomically (~a few seconds blackout). DB data survives because `pgdata/` is a bind-mount.

**After upgrade** read the new release's CHANGELOG for any breaking surface — API contract changes, env var additions, or migrations that change behaviour.

---

## 4. Rolling back

The repository tags every release (`v0.3.x`). To roll back to a known-good version:

```bash
cd netsentinel
git checkout v0.3.5
docker compose up -d --build
```

Migrations are forward-only. If you roll back across a migration that added a column or widened a CHECK constraint, the older binary still works against the newer schema — it just won't use the new column. If you roll back across a migration that **removed** something, you will need to restore from a backup.

---

## 5. Backups

All server state lives in PostgreSQL. A minimal backup script:

```bash
# Daily dump (cron-friendly)
docker compose exec -T db pg_dump -U postgres netsentinel | gzip > "backup-$(date +%F).sql.gz"
```

Restore:

```bash
gunzip -c backup-YYYY-MM-DD.sql.gz \
  | docker compose exec -T db psql -U postgres netsentinel
```

`pgdata/` itself is the canonical storage — snapshot the directory if your volume driver supports it.

---

## 6. Image tagging (for CI pipelines)

Every tagged release on GitHub produces one Docker image per platform:

```
ghcr.io/sounmu/netsentinel-server:<short-sha>
ghcr.io/sounmu/netsentinel-server:latest
```

Pin to `<short-sha>` for reproducible deploys:

```yaml
services:
  server:
    image: ghcr.io/sounmu/netsentinel-server:6a0a9d1
```

---

## 7. Port map (for firewall configuration)

| Port | Who listens | Exposed? |
|---|---|---|
| `3000` | Axum (API + static web) | Yes, via `docker-compose.yml` `ports:` |
| `5432` | PostgreSQL | No — internal to `backend-internal` network |
| `9101` | Agent (default) | Yes, but only on the agent's LAN / tunnel — the server *pulls* |

Nothing else is reachable from outside the stack.
