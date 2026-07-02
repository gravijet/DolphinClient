#!/usr/bin/env bash
# Holt die CI-gebauten nativen Launcher-Binaries aus dem GitHub-Release (privates
# Repo -> per authentifiziertem gh bzw. GH_TOKEN) in den öffentlichen
# Download-Ordner und regeneriert das Manifest.
# Aufruf:  update-downloads.sh [tag]   (Standard: v0.2.0)
set -euo pipefail

TAG="${1:-v0.2.0}"
VERSION="${TAG#v}"
REPO="gravijet/DolphinClient"
DL="/var/www/dolphin.gravijet.net/downloads"
export HOME="${HOME:-/home/benj}"   # gh-Auth liegt unter $HOME/.config/gh

mkdir -p "$DL"

echo "[downloads] Ziehe native Release-Assets für $TAG aus $REPO ..."
gh release download "$TAG" --repo "$REPO" --dir "$DL" --clobber --pattern 'DolphinClient-*'

echo "[downloads] Entferne veraltete Electron-Installer ..."
rm -f "$DL"/*.dmg "$DL"/*.blockmap "$DL"/latest*.yml "$DL"/*.AppImage "$DL"/DolphinClient-Setup-*.exe

echo "[downloads] Regeneriere Manifest ..."
node /opt/dolphinclient/gen-manifest.mjs "$DL" "$VERSION"

chown -R www-data:www-data "$DL"
echo "[downloads] Fertig -> https://dolphin.gravijet.net/downloads/"
