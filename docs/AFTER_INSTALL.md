# After Install — first admin, first host, first agent

This page covers everything that happens *after* `docker compose up -d --build` finishes. Target time: **10 minutes** from a fresh clone to "the dashboard is showing real metrics from a real machine".

If anything below fails, run `./scripts/doctor.sh` — it prints the exact next command for each broken check.

---

## Step 1 — Verify the stack is healthy (30 seconds)

```bash
./scripts/smoke-test.sh
```

Expected output:

```
✅ /api/health responded within 20s
✅ Health payload confirms DB connectivity
✅ Web root / served (static bundle OK)
✅ /api/auth/status — first-time setup is pending (expected on fresh install)
✅ /host/?key=… static shell served

Summary: 5 passed, 0 failed

👉 Next:
    open http://localhost:3000/setup   # create the first admin account
```

Any ❌ line tells you exactly which log to inspect (usually `docker compose logs --tail=60 server`).

---

## Step 2 — Create the first admin (1 minute)

Open **<http://localhost:3000/setup>** in a browser.

The `/setup` page is only reachable while the `users` table is empty. Fill in:

| Field | Notes |
|---|---|
| Username | Any letters/digits. Case-insensitive unique. |
| Password | **≥ 8 characters** + uppercase + lowercase + digit + special character. The frontend validates live; the server enforces the same rules. |
| Confirm password | Must match. |

Click **Create admin** → you are redirected to `/login` → sign in with the same credentials.

> If you navigated to `/setup` but the page says "setup already completed", an admin was created earlier. Delete the `users` table row in Postgres (or just the pgdata volume) to re-enable `/setup`, OR go to `/login` and sign in with the existing credentials.

---

## Step 3 — Add your first host (1 minute, in the browser)

In the navbar, click **Agents** → **+ Add Agent**. Fill in:

| Field | Example | What it means |
|---|---|---|
| `host_key` | `192.168.1.10:9101` | The URL the SERVER will pull metrics from. Format: `host:port`. Must be reachable from the server container. Use the agent machine's LAN IP, not `localhost`. |
| `display_name` | `homeserver` | Shows up in the dashboard. |
| `scrape_interval_secs` | `10` | How often the server polls this host. |
| `load_threshold` | `4.0` | Triggers the high-load alert. |
| `ports` | `80, 443` | Comma-separated; the agent probes each and reports up/down. |
| `containers` | (blank) | Comma-separated Docker container names you want tracked. |

Hit **Save** → the host shows up in `/agents` immediately with status **`pending`**. It turns **`online`** after the first successful scrape — once the agent on that machine is answering.

---

## Step 4 — Install and start the agent on the target machine (5 minutes)

The agent is a single Rust binary. Build and run it on whatever machine you want to monitor.

### 4.1 Copy the shared JWT secret

From the server's `.env`:

```bash
grep ^JWT_SECRET= .env | cut -d= -f2-
```

Keep that value handy.

### 4.2 On the target machine

```bash
# clone once if the repo isn't already there
git clone https://github.com/sounmu/netsentinel.git
cd netsentinel/netsentinel-agent

cp .env.example .env
# Edit .env:
#   JWT_SECRET=<same value you copied from the server>
#   AGENT_PORT=9101       # or anything free on this machine

cargo build --release
./target/release/netsentinel-agent
```

You should see:

```
[INFO] netsentinel-agent 0.3.x listening on 0.0.0.0:9101
```

### 4.3 Confirm the server picks it up

Within one `scrape_interval_secs` cycle, the host on the `/agents` page flips `pending` → `online`, and live metrics start flowing into `/` (Overview) and `/host/?key=<host_key>`.

If it stays `pending`:

- **401 / 403 in agent logs** → `JWT_SECRET` mismatch. Recopy it from the server's `.env`.
- **connection refused** → the server container can't reach `host:port`. Try `docker compose exec server curl -v http://<host>:<port>/metrics` and fix routing (LAN IP, firewall, etc.).
- **bincode decode error** → the server and agent versions are more than one minor apart. Rebuild one of them so the versions match.

---

## Step 5 — (Optional) wire up one notification channel (2 minutes)

In **Alerts** → **Notification channels** → **+ Add channel**:

| Channel | Required field |
|---|---|
| Discord | Webhook URL (from the server settings of your Discord channel) |
| Slack | Incoming Webhook URL |
| Email | SMTP host / port / user / password / from / to |

Hit **Test** on the saved channel to verify delivery. Afterwards, configure thresholds in **Alerts** → **Global defaults** or per host.

---

## Total: ~10 minutes

You now have:

- A web dashboard at `http://localhost:3000`
- One admin account
- One host being scraped every 10 s
- (Optional) one notification channel with a real test notification delivered

---

## Troubleshooting

| Symptom | Most common cause | Fix |
|---|---|---|
| Dashboard shows "No agents registered" | No host added yet | See Step 3 |
| Host stuck at `pending` | Agent not running / JWT mismatch / server can't reach `host:port` | See Step 4.3 |
| Browser shows "Host Not Found" on `/host/?key=…` | You edited the URL to a value that isn't in `/api/hosts` | Register the host first (Step 3) |
| `/setup` returns 404 or redirects to `/login` | Admin already provisioned | Sign in at `/login`; reset via `TRUNCATE users` in Postgres if you truly need a fresh setup |
| `./scripts/smoke-test.sh` fails on `/api/health` | Server container still starting, or DB is unreachable | `docker compose logs --tail=60 server` — look for DATABASE_URL / migration errors |
| `./scripts/doctor.sh` flags `JWT_SECRET is shorter than 32 characters` | Manual edit, or leftover from an older example | Re-run `./scripts/bootstrap.sh --force` to regenerate the secret (⚠️ invalidates every existing agent's JWT — you will need to recopy the new value to each agent) |
| Port `3000` already in use | Another service owns it | Set `SERVER_PORT=XXXX` in `.env` and `docker compose up -d` again |

For production-specific concerns (Cloudflare Tunnel, TLS, custom hostname, reverse proxy), see [`docs/DEPLOYMENT.md`](DEPLOYMENT.md).
