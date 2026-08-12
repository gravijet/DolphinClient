#!/usr/bin/env bash
# =============================================================================
#  Admin-Portal absichern: Cloudflare Zero Trust am Rand + Gate am Ursprung
# =============================================================================
#  Installiert (als root):
#    * die nginx-Konfiguration aus deploy/nginx/ (Snippets + Cloudflare-Geo)
#    * den Token-Prüfdienst  (dolphinclient-access.service, 127.0.0.1:8787)
#    * den Statistik-Erzeuger + Timer (dolphinclient-admin-stats.timer, 10 min)
#
#  Aufruf:
#     sudo deploy/setup-zero-trust.sh                          # installieren
#     sudo deploy/setup-zero-trust.sh --team dein-team.cloudflareaccess.com \
#          --aud <AUD-Tag> [--emails du@example.com,zwei@example.com]
#
#  Ohne --team/--aud wird alles installiert, bleibt aber GESCHLOSSEN (503) —
#  das ist Absicht: lieber zu als versehentlich offen. Anleitung, wie man die
#  Access-Anwendung anlegt und die beiden Werte bekommt: deploy/ZERO-TRUST.md
# =============================================================================
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE=/etc/dolphinclient/access.env
OPT=/opt/dolphinclient
WEBROOT="${DOLPHIN_WEBROOT:-/var/www/dolphinclient.de}"

TEAM=""; AUD=""; EMAILS=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --team)   TEAM="${2:-}"; shift 2 ;;
    --aud)    AUD="${2:-}"; shift 2 ;;
    --emails) EMAILS="${2:-}"; shift 2 ;;
    -h|--help) awk 'NR>1 && /^#/{sub(/^# ?/,""); print; next} NR>1{exit}' "$0"; exit 0 ;;
    *) echo "Unbekannte Option: $1" >&2; exit 1 ;;
  esac
done

[[ $EUID -eq 0 ]] || { echo "Bitte als root ausführen (sudo)." >&2; exit 1; }
command -v node >/dev/null || { echo "node fehlt." >&2; exit 1; }

say() { printf '\n\033[1;36m▸ %s\033[0m\n' "$*"; }
ok()  { printf '  \033[32m✓\033[0m %s\n' "$*"; }

# ---------------------------------------------------------------------------
say "Dienste installieren"
install -d -m 0755 "$OPT"
install -m 0755 "$ROOT/deploy/access-gate.mjs" "$OPT/access-gate.mjs"
install -m 0755 "$ROOT/deploy/admin-stats.mjs" "$OPT/admin-stats.mjs"
ok "$OPT/{access-gate,admin-stats}.mjs"

install -d -m 0750 /etc/dolphinclient
if [[ -n "$TEAM" || -n "$AUD" ]]; then
  # Vorhandene Werte behalten, wenn nur einer der beiden übergeben wurde.
  if [[ -f "$ENV_FILE" ]]; then
    # shellcheck source=/dev/null
    . "$ENV_FILE"
    TEAM="${TEAM:-${ACCESS_TEAM_DOMAIN:-}}"
    AUD="${AUD:-${ACCESS_AUD:-}}"
    EMAILS="${EMAILS:-${ACCESS_ALLOWED_EMAILS:-}}"
  fi
  cat > "$ENV_FILE" <<EOF
# Cloudflare Zero Trust — von deploy/setup-zero-trust.sh geschrieben.
ACCESS_TEAM_DOMAIN=$TEAM
ACCESS_AUD=$AUD
ACCESS_ALLOWED_EMAILS=$EMAILS
ACCESS_LISTEN=127.0.0.1:8787
EOF
  ok "$ENV_FILE (team=$TEAM)"
elif [[ ! -f "$ENV_FILE" ]]; then
  cat > "$ENV_FILE" <<'EOF'
# Cloudflare Zero Trust — NOCH NICHT KONFIGURIERT.
# Solange hier nichts steht, antwortet das Gate auf jede /admin-Anfrage mit 503.
# Werte aus dem Cloudflare-Dashboard (siehe deploy/ZERO-TRUST.md):
ACCESS_TEAM_DOMAIN=
ACCESS_AUD=
# Optional zusätzlich einschränken (Komma-Liste). Leer = jede von Access
# zugelassene Identität darf rein.
ACCESS_ALLOWED_EMAILS=
ACCESS_LISTEN=127.0.0.1:8787
EOF
  ok "$ENV_FILE angelegt (leer — Portal bleibt geschlossen)"
else
  ok "$ENV_FILE bleibt unverändert"
fi
chmod 0640 "$ENV_FILE"

cat > /etc/systemd/system/dolphinclient-access.service <<EOF
[Unit]
Description=DolphinClient — Cloudflare Access token gate (origin side)
Documentation=file://$ROOT/deploy/ZERO-TRUST.md
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
EnvironmentFile=$ENV_FILE
ExecStart=/usr/bin/node $OPT/access-gate.mjs
Restart=always
RestartSec=2
DynamicUser=yes
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=yes
RestrictAddressFamilies=AF_INET AF_INET6
MemoryMax=128M

[Install]
WantedBy=multi-user.target
EOF
ok "dolphinclient-access.service"

cat > /etc/systemd/system/dolphinclient-admin-stats.service <<EOF
[Unit]
Description=DolphinClient — collect admin portal statistics from the nginx logs

[Service]
Type=oneshot
Environment=DOLPHIN_REPO=$ROOT
ExecStart=/usr/bin/node $OPT/admin-stats.mjs $WEBROOT
Nice=10
EOF

cat > /etc/systemd/system/dolphinclient-admin-stats.timer <<'EOF'
[Unit]
Description=DolphinClient — refresh the admin portal statistics every 10 minutes

[Timer]
OnBootSec=2min
OnUnitActiveSec=10min
AccuracySec=30s
Persistent=true

[Install]
WantedBy=timers.target
EOF
ok "dolphinclient-admin-stats.{service,timer}"

# ---------------------------------------------------------------------------
say "nginx-Konfiguration installieren"
backup() { [[ -f "$1" ]] && cp -a "$1" "$1.bak-$(date +%Y%m%d%H%M%S)"; :; }
backup /etc/nginx/snippets/dolphinclient-locations.conf
install -m 0644 "$ROOT/deploy/nginx/dolphinclient-locations.conf" /etc/nginx/snippets/
install -m 0644 "$ROOT/deploy/nginx/dolphinclient-admin.conf" /etc/nginx/snippets/
install -m 0644 "$ROOT/deploy/nginx/dolphinclient-cf-geo.conf" /etc/nginx/conf.d/
ok "snippets/{dolphinclient-locations,dolphinclient-admin}.conf + conf.d/dolphinclient-cf-geo.conf"

install -d -m 0755 "$WEBROOT/admin-data"
chown www-data:www-data "$WEBROOT/admin-data" || true

# ---------------------------------------------------------------------------
say "Starten und prüfen"
systemctl daemon-reload
systemctl enable --now dolphinclient-access.service >/dev/null
systemctl enable --now dolphinclient-admin-stats.timer >/dev/null
systemctl start dolphinclient-admin-stats.service || true
ok "Dienste laufen"

nginx -t
systemctl reload nginx
ok "nginx neu geladen"

sleep 1
HEALTH="$(curl -fsS --max-time 5 http://127.0.0.1:8787/health || echo '{"configured":false}')"
echo "  Gate: $HEALTH"

if grep -q '"configured":true' <<<"$HEALTH"; then
  printf '\n\033[32mFertig.\033[0m Das Admin-Portal ist unter https://dolphinclient.de/admin erreichbar —\n'
  printf 'nur für Identitäten, die deine Cloudflare-Access-Richtlinie zulässt.\n'
else
  printf '\n\033[33mInstalliert, aber noch GESCHLOSSEN (503).\033[0m\n'
  printf 'Lege die Access-Anwendung im Cloudflare-Dashboard an (deploy/ZERO-TRUST.md)\n'
  printf 'und trage sie dann ein:\n\n'
  printf '  sudo deploy/setup-zero-trust.sh --team <dein-team>.cloudflareaccess.com --aud <AUD-Tag>\n\n'
fi
