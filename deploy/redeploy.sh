#!/usr/bin/env bash
# DolphinClient redeploy: baut die Website (statischer Next.js-Export) aus dem
# lokalen Repo-Checkout neu, veröffentlicht sie nach /var/www und lädt nginx
# neu. Als root ausführen. Es gibt kein Backend mehr — das Dashboard verbindet
# sich direkt mit dem laufenden Launcher (lokale Bridge) und liest das
# Download-Manifest.
set -euo pipefail

REPO="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
WEBROOT=/var/www/example.invalid

echo "[1/4] Abhängigkeiten (website)"
cd "$REPO"
npm install -w website --no-audit --no-fund

echo "[2/4] Website bauen (Next.js static export)"
cd "$REPO/website"
npx next build

echo "[3/4] Website veröffentlichen -> $WEBROOT"
mkdir -p "$WEBROOT"
# Alten Build entfernen, ACME-Challenge- und Downloads-Ordner behalten.
find "$WEBROOT" -mindepth 1 -maxdepth 1 ! -name '.well-known' ! -name 'downloads' -exec rm -rf {} +
cp -a "$REPO/website/out/." "$WEBROOT/"
chown -R www-data:www-data "$WEBROOT"

echo "[4/4] nginx neu laden"
nginx -t && systemctl reload nginx

echo "Fertig. https://example.invalid"
