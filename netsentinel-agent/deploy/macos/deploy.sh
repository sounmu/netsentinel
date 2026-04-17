#!/bin/bash
#
# macOS LaunchDaemon deployment for netsentinel-agent.
#
# Run from anywhere:
#   netsentinel-agent/deploy/macos/deploy.sh
#
# Layout it installs:
#   /usr/local/bin/netsentinel-agent                      (binary, 755 root:wheel)
#   /usr/local/etc/netsentinel/.env                       (secrets, 600 root:wheel)
#   /Library/LaunchDaemons/com.sounmu.netsentinel.plist   (daemon spec, 644 root:wheel)
#   /var/log/netsentinel/                                 (logs dir, 755 root:wheel)
#
# Idempotent: running twice is safe. Legacy artifacts from prior naming
# schemes (`com.user.network-monitor`, `com.user.netsentinel`,
# `/usr/local/bin/network-monitor-agent`) are removed on first run.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
AGENT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$AGENT_ROOT"

AGENT_BIN="/usr/local/bin/netsentinel-agent"
AGENT_CONF_DIR="/usr/local/etc/netsentinel"
AGENT_LOG_DIR="/var/log/netsentinel"
PLIST_SRC="$SCRIPT_DIR/com.sounmu.netsentinel.plist"
PLIST_DST="/Library/LaunchDaemons/com.sounmu.netsentinel.plist"

# Every historical plist name we must clean up. Order matters for safety
# (unload before delete) but entries are fully independent.
LEGACY_PLISTS=(
    "/Library/LaunchDaemons/com.user.network-monitor.plist"
    "/Library/LaunchDaemons/com.user.netsentinel.plist"
)
LEGACY_BIN="/usr/local/bin/network-monitor-agent"

# 1. Build
echo "📦 [1/6] Building..."
cargo build --release

# 2. Legacy cleanup (safe no-op if already clean)
echo "🧹 [2/6] Removing legacy artifacts..."
for legacy in "${LEGACY_PLISTS[@]}"; do
    if [ -f "$legacy" ]; then
        sudo launchctl unload "$legacy" 2>/dev/null || true
        sudo rm -f "$legacy"
        echo "   removed $legacy"
    fi
done
if [ -f "$LEGACY_BIN" ]; then
    sudo rm -f "$LEGACY_BIN"
    echo "   removed $LEGACY_BIN"
fi

# 3. Log dir
echo "📂 [3/6] Ensuring log dir at $AGENT_LOG_DIR..."
sudo mkdir -p "$AGENT_LOG_DIR"
sudo chown root:wheel "$AGENT_LOG_DIR"
sudo chmod 755 "$AGENT_LOG_DIR"

# 4. Binary
echo "🚚 [4/6] Installing binary to $AGENT_BIN..."
sudo cp target/release/netsentinel-agent "$AGENT_BIN"
sudo chown root:wheel "$AGENT_BIN"
sudo chmod 755 "$AGENT_BIN"

# 5. Config dir + .env
# .env stays out of the repo; copy it from the project root only if missing
# at the destination. Re-running deploy.sh never clobbers the installed secret.
echo "🔐 [5/6] Ensuring config at $AGENT_CONF_DIR/.env..."
sudo mkdir -p "$AGENT_CONF_DIR"
sudo chown root:wheel "$AGENT_CONF_DIR"
sudo chmod 755 "$AGENT_CONF_DIR"
if [ ! -f "$AGENT_CONF_DIR/.env" ]; then
    if [ -f .env ]; then
        sudo cp .env "$AGENT_CONF_DIR/.env"
        sudo chown root:wheel "$AGENT_CONF_DIR/.env"
        sudo chmod 600 "$AGENT_CONF_DIR/.env"
        echo "   seeded .env from $AGENT_ROOT/.env"
    else
        echo "   ⚠️  no .env installed and no project-root .env to seed from."
        echo "      Create $AGENT_CONF_DIR/.env manually (600 root:wheel) before the daemon will start."
    fi
fi

# 6. LaunchDaemon plist + (re)load
echo "🔄 [6/6] Installing plist and (re)loading daemon..."
sudo cp "$PLIST_SRC" "$PLIST_DST"
sudo chown root:wheel "$PLIST_DST"
sudo chmod 644 "$PLIST_DST"
sudo launchctl unload "$PLIST_DST" 2>/dev/null || true
sudo launchctl load -w "$PLIST_DST"

echo "✅ Deployment completed."
echo "----------------------------------------------------"
echo "👉 Check daemon:   sudo launchctl list | grep netsentinel"
echo "👉 Tail logs:      tail -f $AGENT_LOG_DIR/app.log"
echo "👉 Tail errors:    tail -f $AGENT_LOG_DIR/error.log"
echo "👉 Edit secrets:   sudo vim $AGENT_CONF_DIR/.env  (then: sudo launchctl kickstart -k system/com.sounmu.netsentinel)"
echo "----------------------------------------------------"
