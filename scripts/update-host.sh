#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────
# NetSentinel host monitor — updater
#
# Re-runs `install-host.sh` using the AGENT_AUTH_SECRET (or legacy
# JWT_SECRET), AGENT_PORT, and AGENT_BIND already saved in
# /etc/netsentinel/agent.env. This is the same install-is-update path
# the monitor has always supported, without making the operator paste
# credentials again on every host.
#
# Typical usage on each monitored host:
#
#     curl -fsSL https://raw.githubusercontent.com/sounmu/netsentinel/main/scripts/update-host.sh \
#       | sudo bash                                # → latest release
#     curl -fsSL .../update-host.sh \
#       | sudo bash -s -- --ref v0.5.1             # → pinned tag
#
# Locally next to install-host.sh:
#
#     sudo bash ./scripts/update-host.sh --ref v0.5.1
# ─────────────────────────────────────────────────────────────────────
set -euo pipefail

CONFIG_FILE="${NS_CONFIG_FILE:-/etc/netsentinel/agent.env}"
REF="${NS_REF:-latest}"
INSTALLER_URL="${NS_INSTALLER_URL:-}"
LEGACY_INSTALLER_URL=""
EXTRA_ARGS=()

print_help() {
  cat <<HLP
NetSentinel host updater

Refreshes the host monitor binary (and unit file) using the credentials
already written to ${CONFIG_FILE} by install-host.sh. You almost
never need to pass auth secrets or --port again.

Usage:
  sudo bash update-host.sh [options]

Options:
  --ref TAG               release tag to install [latest]    env: NS_REF
                          use with --build-from-source for branches
  --build-from-source     rebuild via cargo from --ref
                          (requires git + Rust toolchain)
  --installer-url URL     override the install-host.sh URL   env: NS_INSTALLER_URL
                          (default: main for latest, otherwise --ref tag)
  --config-file PATH      host monitor config to read credentials from env: NS_CONFIG_FILE
                          (default: /etc/netsentinel/agent.env)
  -h, --help

Examples:
  # latest release
  sudo bash update-host.sh

  # pin to a specific tag
  sudo bash update-host.sh --ref v0.5.1

  # rebuild from a branch (e.g. testing a fix before release)
  sudo bash update-host.sh --build-from-source --ref dev
HLP
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --ref)               REF="${2:-}"; shift 2 ;;
    --ref=*)             REF="${1#*=}"; shift ;;
    --build-from-source) EXTRA_ARGS+=(--build-from-source); shift ;;
    --installer-url)     INSTALLER_URL="${2:-}"; shift 2 ;;
    --installer-url=*)   INSTALLER_URL="${1#*=}"; shift ;;
    --config-file)       CONFIG_FILE="${2:-}"; shift 2 ;;
    --config-file=*)     CONFIG_FILE="${1#*=}"; shift ;;
    -h|--help)           print_help; exit 0 ;;
    *) echo "❌ Unknown argument: $1" >&2; echo "    Try --help" >&2; exit 2 ;;
  esac
done

if [[ -z "$INSTALLER_URL" ]]; then
  if [[ "$REF" == "latest" ]]; then
    INSTALLER_URL="https://raw.githubusercontent.com/sounmu/netsentinel/main/scripts/install-host.sh"
  else
    INSTALLER_URL="https://raw.githubusercontent.com/sounmu/netsentinel/${REF}/scripts/install-host.sh"
    LEGACY_INSTALLER_URL="https://raw.githubusercontent.com/sounmu/netsentinel/${REF}/scripts/install-agent.sh"
  fi
fi

if [[ $EUID -ne 0 ]]; then
  echo "❌ Must run as root (use sudo)." >&2
  exit 1
fi

if [[ ! -r "$CONFIG_FILE" ]]; then
  cat >&2 <<EOM
❌ ${CONFIG_FILE} not found or unreadable.

Looks like the host monitor was never installed via install-host.sh on this
host (or the config lives elsewhere — pass --config-file).

To bootstrap from scratch instead:
    Open NetSentinel → Hosts → Add Host and copy the generated command.

Legacy/manual fallback:
    curl -fsSL https://raw.githubusercontent.com/sounmu/netsentinel/main/scripts/install-host.sh \\
      | sudo bash -s -- --jwt-secret "<host-auth-secret>"
EOM
  exit 1
fi

# Load saved credentials. CONFIG_FILE format is plain KEY=VALUE.
AGENT_AUTH_SECRET=""
JWT_SECRET=""
AGENT_PORT=""
AGENT_BIND=""
# shellcheck disable=SC1090
. "$CONFIG_FILE"

AUTH_SECRET="${AGENT_AUTH_SECRET:-${JWT_SECRET:-}}"
if [[ -z "${AUTH_SECRET}" ]]; then
  echo "❌ AGENT_AUTH_SECRET missing from ${CONFIG_FILE}." >&2
  exit 1
fi

cmd=(--jwt-secret "$AUTH_SECRET" --ref "$REF")
[[ -n "${AGENT_PORT}" ]] && cmd+=(--port "$AGENT_PORT")
[[ -n "${AGENT_BIND}" ]] && cmd+=(--bind "$AGENT_BIND")
if [[ ${#EXTRA_ARGS[@]} -gt 0 ]]; then
  cmd+=("${EXTRA_ARGS[@]}")
fi

# Prefer a local install-host.sh next to this script (offline-friendly,
# also picks up local edits when developing). Otherwise fetch from the
# pinned URL.
self_dir="$(cd "$(dirname "$0")" 2>/dev/null && pwd || echo "")"
if [[ -n "$self_dir" && -x "${self_dir}/install-host.sh" ]]; then
  echo "▶ Using local installer at ${self_dir}/install-host.sh"
  exec "${self_dir}/install-host.sh" "${cmd[@]}"
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "❌ curl is not on PATH and no local install-host.sh found." >&2
  exit 1
fi

echo "▶ Fetching installer from ${INSTALLER_URL}…"
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
if ! curl -fsSL "$INSTALLER_URL" -o "$tmp"; then
  if [[ -z "$LEGACY_INSTALLER_URL" ]]; then
    exit 1
  fi
  echo "▶ This release predates install-host.sh; using its legacy installer…"
  curl -fsSL "$LEGACY_INSTALLER_URL" -o "$tmp"
fi
chmod 755 "$tmp"
exec bash "$tmp" "${cmd[@]}"
