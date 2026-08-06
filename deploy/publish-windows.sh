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
DL="${DOLPHIN_DOWNLOADS:-/var/www/example.invalid/downloads}"

LAUNCHER="$ROOT/launcher-native/target/x86_64-pc-windows-gnu/release/dolphinclient-launcher.exe"
CLIENT="$ROOT/client-rust/target/x86_64-pc-windows-gnu/release/dolphinclient.exe"

# SKIP_BUILD=1 überspringt das (langsame) Cross-Kompilieren und nimmt die bereits
# vorhandenen Binaries. release.sh baut einmal unprivilegiert und ruft dieses
# Skript dann mit SKIP_BUILD=1 als root auf — so wird nicht doppelt gebaut und
# root muss kein cargo ausführen.
if [[ -z "${SKIP_BUILD:-}" ]]; then
  # shellcheck source=/dev/null
  source "$ROOT/deploy/win-cross-env.sh"
  echo "[win] Launcher wird cross-kompiliert (x86_64-pc-windows-gnu) …"
  ( cd "$ROOT/launcher-native" && cargo build --release --target x86_64-pc-windows-gnu )
  echo "[win] Client wird cross-kompiliert (x86_64-pc-windows-gnu) …"
  ( cd "$ROOT/client-rust" && cargo build --release --target x86_64-pc-windows-gnu )
else
  echo "[win] Bauen übersprungen (SKIP_BUILD) — nutze vorhandene .exe-Dateien."
  [[ -f "$LAUNCHER" ]] || { echo "FEHLER: $LAUNCHER fehlt — erst bauen." >&2; exit 1; }
  [[ -f "$CLIENT"   ]] || { echo "FEHLER: $CLIENT fehlt — erst bauen." >&2; exit 1; }
fi

echo "[win] Installer wird gebaut (NSIS) …"
SETUP="$(mktemp -d)/DolphinClient-Setup-$VERSION.exe"
makensis -DVERSION="$VERSION" \
  -DBINDIR="$ROOT/launcher-native/target/x86_64-pc-windows-gnu/release" \
  -DOUTFILE="$SETUP" \
  "$ROOT/deploy/launcher-installer.nsi"

mkdir -p "$DL"
echo "[win] Installer -> $DL/DolphinClient-Setup-windows-x64.exe"
install -m 0644 "$SETUP" "$DL/DolphinClient-Setup-windows-x64.exe"
# Die nackte .exe bleibt als portable Variante / Fallback verfügbar.
echo "[win] Launcher  -> $DL/DolphinClient-windows-x64.exe"
install -m 0644 "$LAUNCHER" "$DL/DolphinClient-windows-x64.exe"
echo "[win] Client    -> $DL/DolphinClient-Client-windows-x64.exe"
install -m 0644 "$CLIENT" "$DL/DolphinClient-Client-windows-x64.exe"
# Versions-Archiv: ältere Clients bleiben über die Launcher-Versionswahl spielbar.
mkdir -p "$DL/client/$VERSION"
install -m 0644 "$CLIENT" "$DL/client/$VERSION/DolphinClient-Client-windows-x64.exe"

echo "[win] Manifest neu erzeugen ($VERSION) …"
node "$ROOT/deploy/gen-manifest.mjs" "$DL" "$VERSION"

# Changelog zur Laufzeit: die Website lädt downloads/changelog.json direkt (wie
# das Manifest), damit ein neuer Eintrag KEINEN Website-Neubau mehr braucht.
CHANGELOG_SRC="$ROOT/website/app/changelog/changelog.json"
if [[ -f "$CHANGELOG_SRC" ]]; then
  echo "[win] Changelog  -> $DL/changelog.json"
  install -m 0644 "$CHANGELOG_SRC" "$DL/changelog.json"
else
  echo "[win] WARN: $CHANGELOG_SRC fehlt — changelog.json nicht veröffentlicht." >&2
fi

if id www-data >/dev/null 2>&1; then
  chown -R www-data:www-data "$DL" || true
fi
echo "[win] Fertig -> https://example.invalid/downloads/"
