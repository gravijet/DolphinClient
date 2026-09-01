#!/usr/bin/env bash
# Veröffentlicht LOKAL gebaute Binaries im Download-Ordner der Website und
# regeneriert das Manifest. Ersatz für update-downloads.sh, seit nicht mehr auf
# GitHub gebaut wird (kein `gh release download` mehr).
#
# Aufruf:
#   publish-local.sh <version> <launcher-bin> [client-bin]
#
# Beispiel (auf diesem Server, als root, nach `cargo build --release`):
#   deploy/publish-local.sh 0.3.0 \
#     launcher-native/target/release/dolphinclient-launcher \
#     client-rust/target/release/dolphinclient
#
# Läuft die Binaries unter den exakten Asset-Namen ab, die Launcher + Updater
# erwarten. Nur der/die Binaries für DIESES OS werden gesetzt — für ein anderes
# OS das Skript dort erneut ausführen (oder die Datei manuell hinterlegen).
set -euo pipefail

VERSION="${1:?Version fehlt, z. B. 0.3.0}"
LAUNCHER_BIN="${2:?Pfad zum Launcher-Binary fehlt}"
CLIENT_BIN="${3:-}"
DL="${DOLPHIN_DOWNLOADS:-/var/www/dolphinclient.de/downloads}"

# OS + CPU erkennen -> Asset-Namen (müssen mit client.rs und dem Manifest
# übereinstimmen). Intel- und ARM-Binaries werden nie mehr verwechselt.
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) ARCH_TAG="x64" ;;
  arm64|aarch64) ARCH_TAG="arm64" ;;
  *) echo "Unbekannte Architektur: $ARCH" >&2; exit 1 ;;
esac
case "$(uname -s)" in
  Linux*)  LA="DolphinClient-linux-$ARCH_TAG";  CA="DolphinClient-Client-linux-$ARCH_TAG" ;;
  Darwin*) LA="DolphinClient-macos-$ARCH_TAG";  CA="DolphinClient-Client-macos-$ARCH_TAG" ;;
  MINGW*|MSYS*|CYGWIN*) LA="DolphinClient-windows-x64.exe"; CA="DolphinClient-Client-windows-x64.exe" ;;
  *) echo "Unbekanntes OS: $(uname -s)" >&2; exit 1 ;;
esac

mkdir -p "$DL"

echo "[publish] Launcher -> $DL/$LA"
install -m 0755 "$LAUNCHER_BIN" "$DL/$LA"

if [[ -n "$CLIENT_BIN" ]]; then
  echo "[publish] Client   -> $DL/$CA"
  install -m 0755 "$CLIENT_BIN" "$DL/$CA"
  # Versions-Archiv: ältere Clients bleiben über die Launcher-Versionswahl spielbar.
  mkdir -p "$DL/client/$VERSION"
  install -m 0755 "$CLIENT_BIN" "$DL/client/$VERSION/$CA"
fi

echo "[publish] Manifest neu erzeugen ($VERSION) ..."
# gen-manifest.mjs liegt neben diesem Skript (bzw. auf dem Server unter /opt/dolphinclient).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GEN="$SCRIPT_DIR/gen-manifest.mjs"
[[ -f "$GEN" ]] || GEN="/opt/dolphinclient/gen-manifest.mjs"
node "$GEN" "$DL" "$VERSION"

if id www-data >/dev/null 2>&1; then
  chown -R www-data:www-data "$DL" || true
fi
echo "[publish] Fertig -> https://dolphinclient.de/downloads/"
