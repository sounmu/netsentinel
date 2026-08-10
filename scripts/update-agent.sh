#!/usr/bin/env bash
# Deprecated compatibility entry point. Prefer update-host.sh.
set -euo pipefail

self_dir="$(cd "$(dirname "$0")" 2>/dev/null && pwd || echo "")"
if [[ -n "$self_dir" && -x "${self_dir}/update-host.sh" ]]; then
  exec "${self_dir}/update-host.sh" "$@"
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "❌ curl is required to redirect this legacy updater to update-host.sh." >&2
  exit 1
fi

echo "⚠️  update-agent.sh is deprecated; continuing with update-host.sh." >&2
curl -fsSL https://raw.githubusercontent.com/sounmu/netsentinel/main/scripts/update-host.sh | bash -s -- "$@"
