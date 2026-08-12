#!/usr/bin/env bash
# DolphinClient redeploy: baut die Website (statischer Next.js-Export) aus dem
# lokalen Repo-Checkout neu, veröffentlicht sie nach /var/www und lädt nginx
# neu. Als root ausführen. Es gibt kein Backend mehr — das Dashboard verbindet
# sich direkt mit dem laufenden Launcher (lokale Bridge) und liest das
# Download-Manifest.
set -euo pipefail

REPO=/home/benj/DolphinClient
WEBROOT=/var/www/dolphinclient.de

# SKIP_BUILD=1 überspringt npm/next und veröffentlicht nur das vorhandene
# website/out. release.sh baut die Website unprivilegiert und ruft dieses Skript
# dann mit SKIP_BUILD=1 als root auf (so entstehen keine root-eigenen
# node_modules/.next im Repo).
if [[ -z "${SKIP_BUILD:-}" ]]; then
  echo "[1/4] Abhängigkeiten (website)"
  cd "$REPO"
  npm install -w website --no-audit --no-fund

  echo "[2/4] Website bauen (Next.js static export)"
  cd "$REPO/website"
  npx next build
else
  echo "[1-2/4] Bauen übersprungen (SKIP_BUILD) — nutze vorhandenes website/out"
  [[ -d "$REPO/website/out" ]] || { echo "FEHLER: $REPO/website/out fehlt — erst bauen." >&2; exit 1; }
fi

echo "[3/4] Website veröffentlichen -> $WEBROOT"
mkdir -p "$WEBROOT"
# Alten Build entfernen; ACME-Challenge, Downloads und die Admin-Daten (vom
# Timer erzeugt, gehören nicht zum Website-Build) bleiben stehen.
find "$WEBROOT" -mindepth 1 -maxdepth 1 \
  ! -name '.well-known' ! -name 'downloads' ! -name 'admin-data' -exec rm -rf {} +
cp -a "$REPO/website/out/." "$WEBROOT/"
chown -R www-data:www-data "$WEBROOT"

echo "[4/4] nginx neu laden"
nginx -t && systemctl reload nginx

echo "Fertig. https://dolphinclient.de"
