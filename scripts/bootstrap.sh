#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────
# NetSentinel bootstrap
#
# One-shot setup for a fresh clone. Generates the required secrets,
# creates the `.env` file at the repo root, and prints the next step.
# Safe to re-run: refuses to overwrite an existing `.env` without
# `--force`.
#
# Requirements: bash 4+, openssl (for JWT_SECRET and DB password).
#
# Usage:
#   ./scripts/bootstrap.sh            # generate secrets if .env missing
#   ./scripts/bootstrap.sh --force    # overwrite existing .env
# ─────────────────────────────────────────────────────────────────────
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_PATH="${REPO_ROOT}/.env"
EXAMPLE_PATH="${REPO_ROOT}/.env.example"
FORCE=0

for arg in "$@"; do
  case "$arg" in
    --force|-f) FORCE=1 ;;
    --help|-h)
      sed -n '2,14p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "❌ Unknown argument: $arg" >&2
      echo "    Try: $0 --help" >&2
      exit 2
      ;;
  esac
done

# ── pre-flight ──────────────────────────────────────────────────────
if ! command -v openssl >/dev/null 2>&1; then
  echo "❌ openssl not found on PATH." >&2
  echo "    Install it (Debian/Ubuntu: apt install openssl; macOS: brew install openssl)." >&2
  exit 1
fi

if [[ ! -f "$EXAMPLE_PATH" ]]; then
  echo "❌ ${EXAMPLE_PATH} is missing — did the repo clone complete?" >&2
  exit 1
fi

if [[ -f "$ENV_PATH" && $FORCE -eq 0 ]]; then
  echo "ℹ️  ${ENV_PATH} already exists."
  echo "    Re-run with --force to overwrite, or edit it manually."
  exit 0
fi

# ── generate secrets ────────────────────────────────────────────────
JWT_SECRET="$(openssl rand -hex 32)"
POSTGRES_PASSWORD="$(openssl rand -hex 24)"

# ── write .env from the example, substituting placeholders ──────────
python3 - "$EXAMPLE_PATH" "$ENV_PATH" "$JWT_SECRET" "$POSTGRES_PASSWORD" <<'PY'
import sys, re
src, dst, jwt, pw = sys.argv[1:]
content = open(src).read()
content = re.sub(r'^POSTGRES_PASSWORD=.*$', f'POSTGRES_PASSWORD={pw}', content, flags=re.M)
content = re.sub(r'^JWT_SECRET=.*$',         f'JWT_SECRET={jwt}',         content, flags=re.M)
open(dst, 'w').write(content)
PY

chmod 600 "$ENV_PATH"

echo "✅ Wrote ${ENV_PATH} with random secrets (chmod 600)."
echo
echo "    JWT_SECRET has been set. The SAME value is needed in every"
echo "    agent's .env so the server can authenticate scrapes. You can"
echo "    read it back with:"
echo
echo "        grep ^JWT_SECRET= ${ENV_PATH} | cut -d= -f2-"
echo
echo "👉 Next:"
echo "    1. docker compose up -d --build"
echo "    2. ./scripts/smoke-test.sh"
echo "    3. open http://localhost:3000/setup   # create the first admin"
echo
