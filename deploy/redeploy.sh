#!/usr/bin/env bash
# DolphinClient redeploy: baut Website (statischer Export) + Backend aus dem
# lokalen Repo-Checkout neu und startet die Dienste. Als root ausführen.
set -euo pipefail

REPO="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
WEBROOT=/var/www/example.invalid
BACKEND_DEST=/opt/dolphinclient/backend

echo "[1/5] Abhängigkeiten (website + backend)"
cd "$REPO"
ELECTRON_SKIP_BINARY_DOWNLOAD=1 npm install -w website -w backend --no-audit --no-fund

echo "[2/5] Website bauen (Next.js static export)"
cd "$REPO/website"
npx next build

echo "[3/5] Website veröffentlichen -> $WEBROOT"
mkdir -p "$WEBROOT"
# Alten Build entfernen, ACME-Challenge- und Downloads-Ordner behalten.
find "$WEBROOT" -mindepth 1 -maxdepth 1 ! -name '.well-known' ! -name 'downloads' -exec rm -rf {} +
cp -a "$REPO/website/out/." "$WEBROOT/"
chown -R www-data:www-data "$WEBROOT"

echo "[4/5] Backend bauen + veröffentlichen"
cd "$REPO/backend"
npx tsc -p tsconfig.json
rm -rf "$BACKEND_DEST/dist"
cp -a "$REPO/backend/dist" "$BACKEND_DEST/"
cp -a "$REPO/backend/package.json" "$BACKEND_DEST/"
cd "$BACKEND_DEST"
npm install --omit=dev --no-audit --no-fund
chown -R dolphin:dolphin /opt/dolphinclient

echo "[5/5] Dienste neu starten"
systemctl restart dolphin-backend
systemctl reload nginx

echo "Fertig. https://example.invalid"
