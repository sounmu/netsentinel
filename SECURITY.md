# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in this project, please report it responsibly.

**Do NOT open a public GitHub issue for security vulnerabilities.**

Instead, please email the maintainer directly at the email address listed in the GitHub profile: [@sounmu](https://github.com/sounmu)

### What to include

- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

### Response timeline

- **Acknowledgement**: Within 48 hours
- **Initial assessment**: Within 1 week
- **Fix release**: As soon as practical, depending on severity

## Supported Versions

| Version | Supported |
|---------|-----------|
| >= 0.4  | Yes       |
| < 0.4   | No        |

## Security Best Practices for Self-Hosters

- Always deploy behind a reverse proxy (Cloudflare Tunnel, nginx, Caddy) with HTTPS
- Use strong, unique values for `JWT_SECRET` (`openssl rand -hex 32`). It authenticates scrapes of legacy agents only; user sessions are signed with a separate key that the hub generates on first boot and stores in the database (or reads from `USER_JWT_SECRET`), so a monitored host holding `JWT_SECRET` cannot mint dashboard logins
- Hub ↔ agent traffic is plain HTTP. Agent responses are signed (HMAC-SHA256 over the request token and body) so they cannot be altered or replayed in transit, but they are **not encrypted**: anyone on the network path can read the metrics. Put agents on Tailscale / WireGuard (`--network tailscale`) when the network between hub and agents is not trusted
- Once the hub has seen a signed response from an agent it rejects unsigned ones from that host. Downgrading an agent to a build older than response signing therefore needs a re-enrollment (Agents → Add Agent → "Re-enroll an existing host") or removing and re-adding the host
- Ensure the repo-root `.env` is `chmod 600` (bootstrap script does this automatically) — it contains the plaintext `JWT_SECRET`
- Back up `./data/netsentinel.db` regularly; see [`docs/DEPLOYMENT.md`](./docs/DEPLOYMENT.md) §5 for the `VACUUM INTO` pattern. The file contains password hashes, refresh-token hashes, per-agent secrets and (unless `USER_JWT_SECRET` is set) the session signing key, so treat backups with the same care as the live DB
- Keep Docker images updated
- Restrict `ALLOWED_ORIGINS` to your actual domain (not `*`)
- The server container runs as `root` inside the container so the bind-mounted `./data` SQLite file works without host-side `chown` dances. The deployment model assumes the container is fronted by Tailscale / WireGuard / Cloudflare Tunnel, so the only externally reachable surface is the dashboard HTTP port — defended by the in-app auth, SSRF, and input-validation layers documented above. If you intend to expose the container directly to the public internet, drop privileges in your own compose override (`user: "1000:1000"`) and pre-create `./data` with matching ownership
