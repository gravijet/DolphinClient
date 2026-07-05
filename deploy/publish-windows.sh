#!/usr/bin/env bash
# Cross-compiles the Windows launcher + client from this Linux host and publishes
# them to the website downloads dir under the exact asset names the launcher /
# updater expect, then regenerates the manifest.
#
# Usage (from the repo root, as root so it can write /var/www + chown):
#   deploy/publish-windows.sh <version>
#
# Example:
#   sudo deploy/publish-windows.sh 0.4.0
set -euo pipefail

VERSION="${1:?Version fehlt, z. B. 0.4.0}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DL="${DOLPHIN_DOWNLOADS:-/var/www/dolphin.gravijet.net/downloads}"

# shellcheck source=/dev/null
source "$ROOT/deploy/win-cross-env.sh"

echo "[win] Launcher wird cross-kompiliert (x86_64-pc-windows-gnu) …"
( cd "$ROOT/launcher-native" && cargo build --release --target x86_64-pc-windows-gnu )
echo "[win] Client wird cross-kompiliert (x86_64-pc-windows-gnu) …"
( cd "$ROOT/client-rust" && cargo build --release --target x86_64-pc-windows-gnu )

LAUNCHER="$ROOT/launcher-native/target/x86_64-pc-windows-gnu/release/dolphinclient-launcher.exe"
CLIENT="$ROOT/client-rust/target/x86_64-pc-windows-gnu/release/dolphinclient.exe"

mkdir -p "$DL"
echo "[win] Launcher -> $DL/DolphinClient-windows-x64.exe"
install -m 0644 "$LAUNCHER" "$DL/DolphinClient-windows-x64.exe"
echo "[win] Client   -> $DL/DolphinClient-Client-windows-x64.exe"
install -m 0644 "$CLIENT" "$DL/DolphinClient-Client-windows-x64.exe"

echo "[win] Manifest neu erzeugen ($VERSION) …"
node "$ROOT/deploy/gen-manifest.mjs" "$DL" "$VERSION"

if id www-data >/dev/null 2>&1; then
  chown -R www-data:www-data "$DL" || true
fi
echo "[win] Fertig -> https://dolphin.gravijet.net/downloads/"
