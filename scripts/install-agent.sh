#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────
# NetSentinel agent — one-liner installer / updater
#
# Pipes cleanly from curl + bash. Typical usage, on a fresh host:
#
#     curl -fsSL https://raw.githubusercontent.com/sounmu/netsentinel/main/scripts/install-agent.sh \
#       | sudo bash -s -- --server-url "$HUB_URL" --enroll-token "$TOKEN"
#
# The agent is pull-scraped by the hub. New installs exchange a short-lived
# enrollment token for an agent-scoped auth secret; legacy/manual installs
# can still pass --jwt-secret directly. The installer:
#
#   1. Detects OS / CPU architecture.
#   2. Downloads the matching prebuilt binary from GitHub Releases,
#      verifies it with SHA256SUMS, and installs it into
#      ${PREFIX:-/usr/local}/bin.
#   3. Claims the enrollment token against the hub when provided.
#   4. Writes /etc/netsentinel/agent.env (chmod 600) with AGENT_AUTH_SECRET
#      and AGENT_PORT.
#   5. On Linux, drops /etc/systemd/system/netsentinel-agent.service
#      and enables it. On macOS, drops a LaunchDaemon plist.
#   6. Prints the registered host_key.
#
# Safe to re-run; existing binary/config/unit are replaced and the
# service is restarted, so the same command is also the update path.
# Pass --build-from-source if you intentionally want the older cargo
# install path for a branch or local fork.
# ─────────────────────────────────────────────────────────────────────
set -euo pipefail

# ── defaults ────────────────────────────────────────────────────────
JWT_SECRET=""
SERVER_URL=""
ENROLL_TOKEN=""
AGENT_PORT="9101"
BIND_ADDR="0.0.0.0"
BIND_EXPLICIT=0
NETWORK_MODE="lan"
PREFIX="/usr/local"
REPO_URL="https://github.com/sounmu/netsentinel.git"
REF="latest"
SERVICE_NAME="netsentinel-agent"
BIN_NAME="netsentinel-agent"
WRAPPER_NAME="netsentinel-agent-wrapper"
CONFIG_DIR="/etc/netsentinel"
CONFIG_FILE="${CONFIG_DIR}/agent.env"
LOG_DIR="/var/log/netsentinel-agent"
UNINSTALL=0
BUILD_FROM_SOURCE=0

# ── arg parse ───────────────────────────────────────────────────────
print_help() {
  cat <<'HLP'
NetSentinel agent installer / updater

Usage:
  sudo bash install-agent.sh [options]

Options:
  --server-url URL      NetSentinel hub URL               env: NS_SERVER_URL
  --enroll-token VALUE  one-time enrollment token          env: NS_ENROLL_TOKEN
  --jwt-secret VALUE    legacy/manual agent auth secret    env: NS_JWT_SECRET
  --network MODE        lan or tailscale [lan]             env: NS_NETWORK_MODE
  --port N              port the agent listens on [9101]   env: NS_AGENT_PORT
  --bind ADDR           bind address [0.0.0.0]             env: NS_BIND_ADDR
  --prefix DIR          install prefix [/usr/local]        env: NS_PREFIX
  --repo URL            git repo to build from             env: NS_REPO_URL
  --ref TAG             release tag to install [latest]    env: NS_REF
                         use with --build-from-source for branches
  --build-from-source   build via cargo from --repo/--ref
  --uninstall           stop service + remove binary / unit / config
  --help

Recommended path from the NetSentinel UI:
  curl -fsSL .../install-agent.sh | sudo -E bash -s -- \
    --server-url "https://hub.example.com" \
    --enroll-token "nsenr_..." \
    --network lan \
    --port 9101

Tailscale-only exposure example:
  curl -fsSL .../install-agent.sh | sudo -E bash -s -- \
    --server-url "https://hub.example.com" \
    --enroll-token "nsenr_..." \
    --network tailscale \
    --port 9101

Build an unreleased branch from source:
  curl -fsSL .../install-agent.sh | sudo -E bash -s -- \
    --server-url "http://hub:3000" --enroll-token "nsenr_..." \
    --build-from-source --ref main

Without sudo, the script can only run as root or will refuse.
HLP
}

# env var fallbacks (lets operators pass values through `sudo -E`)
[[ -n "${NS_JWT_SECRET:-}" ]] && JWT_SECRET="$NS_JWT_SECRET"
[[ -n "${NS_SERVER_URL:-}" ]] && SERVER_URL="$NS_SERVER_URL"
[[ -n "${NS_ENROLL_TOKEN:-}" ]] && ENROLL_TOKEN="$NS_ENROLL_TOKEN"
[[ -n "${NS_AGENT_PORT:-}" ]] && AGENT_PORT="$NS_AGENT_PORT"
if [[ -n "${NS_BIND_ADDR:-}" ]]; then
  BIND_ADDR="$NS_BIND_ADDR"
  BIND_EXPLICIT=1
fi
[[ -n "${NS_NETWORK_MODE:-}" ]] && NETWORK_MODE="$NS_NETWORK_MODE"
[[ -n "${NS_PREFIX:-}"     ]] && PREFIX="$NS_PREFIX"
[[ -n "${NS_REPO_URL:-}"   ]] && REPO_URL="$NS_REPO_URL"
[[ -n "${NS_REF:-}"        ]] && REF="$NS_REF"
case "${NS_BUILD_FROM_SOURCE:-}" in
  1|true|TRUE|yes|YES) BUILD_FROM_SOURCE=1 ;;
esac

while [[ $# -gt 0 ]]; do
  case "$1" in
    --jwt-secret) JWT_SECRET="${2:-}"; shift 2 ;;
    --jwt-secret=*) JWT_SECRET="${1#*=}"; shift ;;
    --server-url) SERVER_URL="${2:-}"; shift 2 ;;
    --server-url=*) SERVER_URL="${1#*=}"; shift ;;
    --enroll-token) ENROLL_TOKEN="${2:-}"; shift 2 ;;
    --enroll-token=*) ENROLL_TOKEN="${1#*=}"; shift ;;
    --network)    NETWORK_MODE="${2:-}"; shift 2 ;;
    --network=*)  NETWORK_MODE="${1#*=}"; shift ;;
    --port)       AGENT_PORT="${2:-}"; shift 2 ;;
    --port=*)     AGENT_PORT="${1#*=}"; shift ;;
    --bind)       BIND_ADDR="${2:-}"; BIND_EXPLICIT=1; shift 2 ;;
    --bind=*)     BIND_ADDR="${1#*=}"; BIND_EXPLICIT=1; shift ;;
    --prefix)     PREFIX="${2:-}"; shift 2 ;;
    --prefix=*)   PREFIX="${1#*=}"; shift ;;
    --repo)       REPO_URL="${2:-}"; shift 2 ;;
    --repo=*)     REPO_URL="${1#*=}"; shift ;;
    --ref)        REF="${2:-}"; shift 2 ;;
    --ref=*)      REF="${1#*=}"; shift ;;
    --build-from-source) BUILD_FROM_SOURCE=1; shift ;;
    --uninstall)  UNINSTALL=1; shift ;;
    --help|-h)    print_help; exit 0 ;;
    *) echo "❌ Unknown argument: $1" >&2; echo "    Try --help" >&2; exit 2 ;;
  esac
done

# ── must run as root (systemctl / /usr/local writes) ────────────────
if [[ $EUID -ne 0 ]]; then
  echo "❌ This installer must run as root (use sudo)." >&2
  echo "    Example: curl -fsSL .../install-agent.sh | sudo bash -s -- --server-url URL --enroll-token TOKEN" >&2
  exit 1
fi

# ── uninstall path ──────────────────────────────────────────────────
os="$(uname -s)"
if [[ $UNINSTALL -eq 1 ]]; then
  echo "▶ Uninstalling ${SERVICE_NAME}…"
  case "$os" in
    Linux)
      systemctl stop "${SERVICE_NAME}" 2>/dev/null || true
      systemctl disable "${SERVICE_NAME}" 2>/dev/null || true
      rm -f "/etc/systemd/system/${SERVICE_NAME}.service"
      systemctl daemon-reload
      ;;
    Darwin)
      launchctl unload "/Library/LaunchDaemons/dev.netsentinel.agent.plist" 2>/dev/null || true
      launchctl unload "/Library/LaunchDaemons/com.sounmu.netsentinel.plist" 2>/dev/null || true
      rm -f "/Library/LaunchDaemons/dev.netsentinel.agent.plist"
      rm -f "/Library/LaunchDaemons/com.sounmu.netsentinel.plist"
      ;;
  esac
  rm -f "${PREFIX}/bin/${BIN_NAME}"
  rm -f "${PREFIX}/bin/${WRAPPER_NAME}"
  rm -rf "${CONFIG_DIR}"
  rm -rf "/usr/local/etc/netsentinel"
  rm -rf "${LOG_DIR}"
  echo "✅ Uninstalled."
  exit 0
fi

# ── network helpers ─────────────────────────────────────────────────
detect_lan_ip() {
  local ip=""
  if command -v hostname >/dev/null 2>&1 && hostname -I >/dev/null 2>&1; then
    ip="$(hostname -I | awk '{print $1}')"
  fi
  if [[ -z "${ip}" ]] && command -v ipconfig >/dev/null 2>&1; then
    ip="$(ipconfig getifaddr en0 2>/dev/null || true)"
  fi
  [[ -z "${ip}" ]] && ip="127.0.0.1"
  echo "$ip"
}

detect_tailscale_ip() {
  if ! command -v tailscale >/dev/null 2>&1; then
    echo "❌ --network tailscale requires the tailscale CLI on this host." >&2
    echo "    Install and log in to Tailscale first, then re-run this command." >&2
    exit 1
  fi
  local ip
  ip="$(tailscale ip -4 2>/dev/null | awk 'NR == 1 { print }')"
  if [[ -z "$ip" ]]; then
    echo "❌ Tailscale is installed but this host has no Tailscale IPv4 address." >&2
    echo "    Run 'tailscale up' and confirm the hub can reach this node." >&2
    exit 1
  fi
  echo "$ip"
}

json_escape() {
  sed 's/\\/\\\\/g; s/"/\\"/g' <<<"$1" | tr -d '\n'
}

extract_json_string() {
  local key="$1"
  sed -n "s/.*\"${key}\"[[:space:]]*:[[:space:]]*\"\\([^\"]*\\)\".*/\\1/p"
}

# ── validate required args ──────────────────────────────────────────
case "$NETWORK_MODE" in
  lan|tailscale) ;;
  *)
    echo "❌ Invalid --network '$NETWORK_MODE'. Use 'lan' or 'tailscale'." >&2
    exit 1
    ;;
esac

if [[ "$NETWORK_MODE" == "tailscale" && "$BIND_EXPLICIT" -eq 0 ]]; then
  BIND_ADDR="$(detect_tailscale_ip)"
fi

SERVER_URL="${SERVER_URL%/}"
if [[ -n "$ENROLL_TOKEN" || -n "$SERVER_URL" ]]; then
  if [[ -z "$ENROLL_TOKEN" || -z "$SERVER_URL" ]]; then
    echo "❌ --server-url and --enroll-token must be provided together." >&2
    exit 1
  fi
elif [[ -z "$JWT_SECRET" ]]; then
  echo "❌ --enroll-token is required for new installs." >&2
  echo "    Open NetSentinel → Agents → Add Agent and copy the generated command." >&2
  echo "    Legacy/manual installs may pass --jwt-secret directly." >&2
  exit 1
fi

if [[ -n "$JWT_SECRET" && ${#JWT_SECRET} -lt 32 ]]; then
  echo "❌ Agent auth secret is only ${#JWT_SECRET} chars; it must be ≥ 32." >&2
  exit 1
fi
if ! [[ "$AGENT_PORT" =~ ^[0-9]+$ ]] || (( AGENT_PORT < 1 || AGENT_PORT > 65535 )); then
  echo "❌ Invalid --port '$AGENT_PORT'. Must be 1–65535." >&2
  exit 1
fi

mkdir -p "${PREFIX}/bin" "${CONFIG_DIR}" "${LOG_DIR}"
chmod 755 "${PREFIX}/bin"
chmod 755 "${LOG_DIR}"

# ── binary install helpers ──────────────────────────────────────────
github_repo_path() {
  case "$REPO_URL" in
    https://github.com/*)
      local path="${REPO_URL#https://github.com/}"
      echo "${path%.git}"
      ;;
    git@github.com:*)
      local path="${REPO_URL#git@github.com:}"
      echo "${path%.git}"
      ;;
    *)
      echo "sounmu/netsentinel"
      ;;
  esac
}

release_download_base() {
  local repo_path
  repo_path="$(github_repo_path)"
  if [[ "$REF" == "latest" || "$REF" == "main" ]]; then
    echo "https://github.com/${repo_path}/releases/latest/download"
  else
    echo "https://github.com/${repo_path}/releases/download/${REF}"
  fi
}

detect_release_platform() {
  local kernel arch
  kernel="$(uname -s)"
  arch="$(uname -m)"
  case "${kernel}:${arch}" in
    Linux:x86_64|Linux:amd64) echo "linux-amd64" ;;
    Linux:aarch64|Linux:arm64) echo "linux-arm64" ;;
    Darwin:x86_64|Darwin:amd64) echo "darwin-amd64" ;;
    Darwin:arm64|Darwin:aarch64) echo "darwin-arm64" ;;
    *)
      echo "❌ Unsupported platform: ${kernel}/${arch}" >&2
      echo "    Use --build-from-source on this host, or request a release asset." >&2
      exit 1
      ;;
  esac
}

verify_checksum() {
  local checksum_file="$1"
  local asset="$2"
  local line
  line="$(awk -v asset="$asset" '$2 == asset { print; found = 1 } END { exit found ? 0 : 1 }' "$checksum_file" || true)"
  if [[ -z "$line" ]]; then
    echo "❌ ${asset} is missing from SHA256SUMS." >&2
    exit 1
  fi

  if command -v sha256sum >/dev/null 2>&1; then
    printf '%s\n' "$line" | sha256sum -c -
  elif command -v shasum >/dev/null 2>&1; then
    printf '%s\n' "$line" | shasum -a 256 -c -
  else
    echo "❌ neither sha256sum nor shasum is available for checksum verification." >&2
    exit 1
  fi
}

install_prebuilt_binary() {
  for tool in curl tar; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      echo "❌ '$tool' is not on PATH." >&2
      echo "    Install it first, or retry with --build-from-source." >&2
      exit 1
    fi
  done

  local platform asset base tmpdir
  platform="$(detect_release_platform)"
  asset="netsentinel-agent-${platform}.tar.gz"
  base="$(release_download_base)"
  tmpdir="$(mktemp -d)"

  echo "▶ Downloading prebuilt ${BIN_NAME} (${platform})…"
  echo "    release: ${REF}  asset: ${asset}"
  curl -fsSL --retry 3 -o "${tmpdir}/${asset}" "${base}/${asset}"
  curl -fsSL --retry 3 -o "${tmpdir}/SHA256SUMS" "${base}/SHA256SUMS"

  (
    cd "$tmpdir"
    verify_checksum "SHA256SUMS" "$asset"
    tar -xzf "$asset"
  )

  if [[ ! -f "${tmpdir}/${BIN_NAME}" ]]; then
    echo "❌ ${asset} did not contain ${BIN_NAME} at the archive root." >&2
    exit 1
  fi

  install -m 755 "${tmpdir}/${BIN_NAME}" "${PREFIX}/bin/${BIN_NAME}"
  rm -rf "$tmpdir"
  echo "✅ Installed ${PREFIX}/bin/${BIN_NAME}"
}

install_from_source() {
  if ! command -v git >/dev/null 2>&1; then
    cat >&2 <<'EOM'
❌ git is not on PATH.

Install git and try again:
    Debian/Ubuntu:  apt install -y git
    Fedora/RHEL:    dnf install -y git
    Alpine:         apk add git
EOM
    exit 1
  fi

  if ! command -v cargo >/dev/null 2>&1; then
    cat >&2 <<'EOM'
❌ cargo (the Rust toolchain) is not on PATH.

This installer was asked to build the agent from source via
`cargo install --path` after cloning the NetSentinel repository.
Install rustup and try again:

    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
    source "$HOME/.cargo/env"

If you prefer a packaged Rust, your distro may have it:
    Debian/Ubuntu:  apt install cargo
    Alpine:         apk add cargo
    Fedora:         dnf install cargo
EOM
    exit 1
  fi

  echo "▶ Building ${BIN_NAME} via cargo (this takes a few minutes on first run)…"
  echo "    repo: ${REPO_URL}  ref: ${REF}  → ${PREFIX}/bin/${BIN_NAME}"
  local tmpdir
  tmpdir="$(mktemp -d)"

  if ! git clone --depth 1 --branch "$REF" "$REPO_URL" "$tmpdir/repo" >/dev/null 2>&1; then
    git clone "$REPO_URL" "$tmpdir/repo" >/dev/null
    git -C "$tmpdir/repo" checkout "$REF" >/dev/null
  fi

  if ! cargo install --locked --path "$tmpdir/repo/agent" --root "$PREFIX"; then
    cat >&2 <<'EOM'
❌ `cargo install` failed.

Common causes + fixes:
  • missing system libs → Debian/Ubuntu: apt install -y build-essential pkg-config libssl-dev
                          Fedora/RHEL:    dnf groupinstall "Development Tools" && dnf install openssl-devel
                          Alpine:         apk add build-base openssl-dev pkgconfig
  • out of memory       → the compile needs ~1 GB. Add swap or use a bigger VM.
  • Rust too old        → run `rustup update stable`.
EOM
    exit 1
  fi

  rm -rf "$tmpdir"
}

# ── install the binary ──────────────────────────────────────────────
if [[ "$BUILD_FROM_SOURCE" -eq 1 ]]; then
  install_from_source
else
  install_prebuilt_binary
fi

# ── claim enrollment token ──────────────────────────────────────────
advertise_addr="$BIND_ADDR"
if [[ "$advertise_addr" == "0.0.0.0" || "$advertise_addr" == "::" ]]; then
  if [[ "$NETWORK_MODE" == "tailscale" ]]; then
    advertise_addr="$(detect_tailscale_ip)"
  else
    advertise_addr="$(detect_lan_ip)"
  fi
fi
HOST_KEY="${advertise_addr}:${AGENT_PORT}"
DISPLAY_NAME="$(hostname 2>/dev/null || echo "$HOST_KEY")"
ENROLLED_HOST_KEY=""

if [[ -n "$ENROLL_TOKEN" ]]; then
  echo "▶ Claiming enrollment token with ${SERVER_URL}…"
  host_key_json="$(json_escape "$HOST_KEY")"
  display_name_json="$(json_escape "$DISPLAY_NAME")"
  token_json="$(json_escape "$ENROLL_TOKEN")"
  network_json="$(json_escape "$NETWORK_MODE")"
  claim_payload="{\"token\":\"${token_json}\",\"host_key\":\"${host_key_json}\",\"display_name\":\"${display_name_json}\",\"network_mode\":\"${network_json}\"}"
  claim_response="$(curl -fsSL --retry 3 \
    -X POST \
    -H "Content-Type: application/json" \
    -d "$claim_payload" \
    "${SERVER_URL}/api/agent-enrollments/claim")"
  JWT_SECRET="$(printf '%s' "$claim_response" | extract_json_string "agent_auth_secret")"
  ENROLLED_HOST_KEY="$(printf '%s' "$claim_response" | extract_json_string "host_key")"
  if [[ -z "$JWT_SECRET" || ${#JWT_SECRET} -lt 32 ]]; then
    echo "❌ Enrollment claim succeeded but did not return a valid agent auth secret." >&2
    exit 1
  fi
  [[ -n "$ENROLLED_HOST_KEY" ]] && HOST_KEY="$ENROLLED_HOST_KEY"
  echo "✅ Enrollment claimed for ${HOST_KEY}"
fi

# ── write agent config ──────────────────────────────────────────────
cat > "${CONFIG_FILE}" <<EOF
# Managed by scripts/install-agent.sh — re-run with different flags to replace.
# AGENT_AUTH_SECRET is the preferred key. JWT_SECRET is kept as a compatibility
# alias for older pinned agent binaries.
AGENT_AUTH_SECRET=${JWT_SECRET}
JWT_SECRET=${JWT_SECRET}
AGENT_PORT=${AGENT_PORT}
AGENT_BIND=${BIND_ADDR}
EOF
chmod 600 "${CONFIG_FILE}"
echo "✅ Wrote ${CONFIG_FILE} (chmod 600)"

# ── install service ────────────────────────────────────────────────
case "$os" in
  Linux)
    if ! command -v systemctl >/dev/null 2>&1; then
      echo "⚠️  systemd not found — the binary is installed at ${PREFIX}/bin/${BIN_NAME}"
      echo "    Run it manually: AGENT_AUTH_SECRET=… ${PREFIX}/bin/${BIN_NAME}"
    else
      unit_path="/etc/systemd/system/${SERVICE_NAME}.service"
      cat > "$unit_path" <<EOF
[Unit]
Description=NetSentinel agent (pull-scraped monitoring agent)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
EnvironmentFile=${CONFIG_FILE}
ExecStart=${PREFIX}/bin/${BIN_NAME}
Restart=on-failure
RestartSec=5
# Hardening: the agent only needs to listen on a TCP port and read
# its own env file.
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ReadWritePaths=${LOG_DIR}

[Install]
WantedBy=multi-user.target
EOF
      systemctl daemon-reload
      systemctl enable "${SERVICE_NAME}.service" >/dev/null
      systemctl restart "${SERVICE_NAME}.service"
      sleep 1
      if systemctl is-active --quiet "${SERVICE_NAME}.service"; then
        echo "✅ systemd service ${SERVICE_NAME} is active"
      else
        echo "⚠️  service failed to start — inspect with:"
        echo "    sudo journalctl -u ${SERVICE_NAME} --since '1 min ago'"
      fi
    fi
    ;;
  Darwin)
    plist="/Library/LaunchDaemons/dev.netsentinel.agent.plist"
    wrapper="${PREFIX}/bin/${WRAPPER_NAME}"
    # Retire the legacy manual macOS installer artifacts if this unified
    # installer is used on a machine that previously ran deploy/macos.
    launchctl unload "/Library/LaunchDaemons/com.sounmu.netsentinel.plist" 2>/dev/null || true
    rm -f "/Library/LaunchDaemons/com.sounmu.netsentinel.plist"
    cat > "$wrapper" <<EOF
#!/bin/sh
set -a
. "${CONFIG_FILE}"
set +a
exec "${PREFIX}/bin/${BIN_NAME}"
EOF
    chmod 755 "$wrapper"
    cat > "$plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>dev.netsentinel.agent</string>
  <key>ProgramArguments</key>
  <array>
    <string>${wrapper}</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>/var/log/netsentinel-agent.log</string>
  <key>StandardErrorPath</key><string>/var/log/netsentinel-agent.log</string>
</dict>
</plist>
EOF
    chmod 644 "$plist"
    launchctl unload "$plist" 2>/dev/null || true
    launchctl load "$plist"
    echo "✅ launchd daemon dev.netsentinel.agent is running"
    ;;
  *)
    echo "⚠️  OS '$os' is not wired for automatic service management."
    echo "    Binary: ${PREFIX}/bin/${BIN_NAME}"
    echo "    Run manually with:"
    echo "        AGENT_AUTH_SECRET=… AGENT_PORT=${AGENT_PORT} ${PREFIX}/bin/${BIN_NAME}"
    ;;
esac

# ── print pairing info ──────────────────────────────────────────────
if [[ -n "$ENROLL_TOKEN" ]]; then
  next_step="The hub has registered ${HOST_KEY}; it should flip to 'online' within one scrape cycle."
else
  next_step="Legacy/manual install: open Agents → + Add Agent on the hub and enter host_key ${HOST_KEY}."
fi

cat <<EOM

─────────────────────────────────────────────────────────────────────
✅ Agent installed and running.

${next_step}

Manage this agent:
    sudo systemctl status  ${SERVICE_NAME}      # (Linux)
    sudo launchctl list   dev.netsentinel.agent # (macOS)

Update this agent:
    Re-run the same installer command with --ref <release-tag>.
    Use --build-from-source --ref <branch> for unreleased code.

Remove this agent:
    sudo $(realpath "$0") --uninstall

─────────────────────────────────────────────────────────────────────
EOM
