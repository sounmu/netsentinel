#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────
# NetSentinel host monitor — uninstaller
#
# Standalone counterpart to `install-host.sh`. Stops the host monitor
# service and removes its binary, config, unit file, and log dir.
#
# Curl-able usage:
#
#     curl -fsSL https://raw.githubusercontent.com/sounmu/netsentinel/main/scripts/remove-host.sh \
#       | sudo bash
#
# Equivalent to `install-host.sh --uninstall` and shares its layout
# defaults; pick whichever you have on hand.
# ─────────────────────────────────────────────────────────────────────
set -euo pipefail

PREFIX="/usr/local"
SERVICE_NAME="netsentinel-agent"
BIN_NAME="netsentinel-agent"
WRAPPER_NAME="netsentinel-agent-wrapper"
CONFIG_DIR="/etc/netsentinel"
LOG_DIR="/var/log/netsentinel-agent"
ASSUME_YES=0

print_help() {
  cat <<'HLP'
NetSentinel host uninstaller

Stops the host monitoring service (systemd on Linux, launchd on macOS) and
removes its binary, config (chmod 600 auth secret), service unit, and
log dir.

Usage:
  sudo bash remove-host.sh [options]

Options:
  --prefix DIR     install prefix used at install time [/usr/local]  env: NS_PREFIX
  -y, --yes        skip the interactive confirmation
  -h, --help

Examples:
  # interactive
  sudo bash remove-host.sh

  # one-liner, no prompt
  curl -fsSL https://raw.githubusercontent.com/sounmu/netsentinel/main/scripts/remove-host.sh \
    | sudo bash -s -- -y
HLP
}

[[ -n "${NS_PREFIX:-}" ]] && PREFIX="$NS_PREFIX"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix)   PREFIX="${2:-}"; shift 2 ;;
    --prefix=*) PREFIX="${1#*=}"; shift ;;
    -y|--yes)   ASSUME_YES=1; shift ;;
    -h|--help)  print_help; exit 0 ;;
    *) echo "❌ Unknown argument: $1" >&2; echo "    Try --help" >&2; exit 2 ;;
  esac
done

if [[ $EUID -ne 0 ]]; then
  echo "❌ Must run as root (use sudo)." >&2
  exit 1
fi

cat <<EOM
About to remove the NetSentinel host monitor:
  • stop ${SERVICE_NAME} (systemd or launchd)
  • rm ${PREFIX}/bin/${BIN_NAME}
  • rm ${PREFIX}/bin/${WRAPPER_NAME}        (macOS only)
  • rm -rf ${CONFIG_DIR}                    (contains the host auth secret)
  • rm -rf ${LOG_DIR}
EOM
if [[ $ASSUME_YES -ne 1 ]]; then
  read -r -p "Proceed? [y/N] " ans
  case "$ans" in
    y|Y|yes|YES) ;;
    *) echo "Aborted."; exit 0 ;;
  esac
fi

os="$(uname -s)"
case "$os" in
  Linux)
    if command -v systemctl >/dev/null 2>&1; then
      systemctl stop "${SERVICE_NAME}" 2>/dev/null || true
      systemctl disable "${SERVICE_NAME}" 2>/dev/null || true
      rm -f "/etc/systemd/system/${SERVICE_NAME}.service"
      systemctl daemon-reload
    fi
    ;;
  Darwin)
    launchctl unload "/Library/LaunchDaemons/dev.netsentinel.agent.plist" 2>/dev/null || true
    launchctl unload "/Library/LaunchDaemons/com.sounmu.netsentinel.plist" 2>/dev/null || true
    rm -f "/Library/LaunchDaemons/dev.netsentinel.agent.plist"
    rm -f "/Library/LaunchDaemons/com.sounmu.netsentinel.plist"
    ;;
  *)
    echo "⚠️  Unrecognised OS '$os' — removing files only, no service unit to stop."
    ;;
esac

rm -f "${PREFIX}/bin/${BIN_NAME}"
rm -f "${PREFIX}/bin/${WRAPPER_NAME}"
rm -rf "${CONFIG_DIR}"
rm -rf "/usr/local/etc/netsentinel"
rm -rf "${LOG_DIR}"

echo "✅ Host monitor removed."
