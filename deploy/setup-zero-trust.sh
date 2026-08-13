#!/usr/bin/env bash
# =============================================================================
#  Secure the admin portal: Cloudflare Zero Trust at the edge + origin gate
# =============================================================================
#  Installs (as root):
#    * the nginx configuration from deploy/nginx/ (snippets + Cloudflare geo)
#    * the token gate         (dolphinclient-access.service, 127.0.0.1:8787)
#    * the stats collector + timer (dolphinclient-admin-stats.timer, 10 min)
#
#  Usage:
#     sudo deploy/setup-zero-trust.sh                          # install only
#     sudo deploy/setup-zero-trust.sh --team your-team.cloudflareaccess.com \
#          --aud <AUD tag> [--emails you@example.com,user@example.invalid]
#
#  Without --team/--aud everything is installed but stays SEALED (503) — that
#  is deliberate: better shut than accidentally open. How to create the Access
#  application and get both values: deploy/ZERO-TRUST.md
# =============================================================================
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE=/etc/dolphinclient/access.env
OPT=/opt/dolphinclient
WEBROOT="${DOLPHIN_WEBROOT:-/var/www/example.invalid}"

TEAM=""; AUD=""; EMAILS=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --team)   TEAM="${2:-}"; shift 2 ;;
    --aud)    AUD="${2:-}"; shift 2 ;;
    --emails) EMAILS="${2:-}"; shift 2 ;;
    -h|--help) awk 'NR>1 && /^#/{sub(/^# ?/,""); print; next} NR>1{exit}' "$0"; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

[[ $EUID -eq 0 ]] || { echo "Please run as root (sudo)." >&2; exit 1; }
command -v node >/dev/null || { echo "node is missing." >&2; exit 1; }

say() { printf '\n\033[1;36m▸ %s\033[0m\n' "$*"; }
ok()  { printf '  \033[32m✓\033[0m %s\n' "$*"; }

# ---------------------------------------------------------------------------
say "Installing the services"
install -d -m 0755 "$OPT"
install -m 0644 "$ROOT/deploy/access-verify.mjs" "$OPT/access-verify.mjs"
install -m 0755 "$ROOT/deploy/access-gate.mjs" "$OPT/access-gate.mjs"
install -m 0755 "$ROOT/deploy/admin-api.mjs" "$OPT/admin-api.mjs"
install -m 0755 "$ROOT/deploy/admin-stats.mjs" "$OPT/admin-stats.mjs"
ok "$OPT/{access-verify,access-gate,admin-api,admin-stats}.mjs"

install -d -m 0750 /etc/dolphinclient
if [[ -n "$TEAM" || -n "$AUD" ]]; then
  # Keep existing values when only one of the two was passed.
  if [[ -f "$ENV_FILE" ]]; then
    # shellcheck source=/dev/null
    . "$ENV_FILE"
    TEAM="${TEAM:-${ACCESS_TEAM_DOMAIN:-}}"
    AUD="${AUD:-${ACCESS_AUD:-}}"
    EMAILS="${EMAILS:-${ACCESS_ALLOWED_EMAILS:-}}"
  fi
  cat > "$ENV_FILE" <<EOF
# Cloudflare Zero Trust — written by deploy/setup-zero-trust.sh.
ACCESS_TEAM_DOMAIN=$TEAM
ACCESS_AUD=$AUD
ACCESS_ALLOWED_EMAILS=$EMAILS
ACCESS_LISTEN=127.0.0.1:8787
EOF
  ok "$ENV_FILE (team=$TEAM)"
elif [[ ! -f "$ENV_FILE" ]]; then
  cat > "$ENV_FILE" <<'EOF'
# Cloudflare Zero Trust — NOT CONFIGURED YET.
# While this is empty the gate answers every /admin request with 503.
# Values from the Cloudflare dashboard (see deploy/ZERO-TRUST.md):
ACCESS_TEAM_DOMAIN=
ACCESS_AUD=
# Optionally restrict further (comma list). Empty = every identity Access
# lets through may enter.
ACCESS_ALLOWED_EMAILS=
ACCESS_LISTEN=127.0.0.1:8787
EOF
  ok "$ENV_FILE created (empty — the portal stays sealed)"
else
  ok "$ENV_FILE left unchanged"
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

cat > /etc/systemd/system/dolphinclient-admin-api.service <<EOF
[Unit]
Description=DolphinClient — admin portal write API (changelog + screenshots)
Documentation=file://$ROOT/deploy/ZERO-TRUST.md
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
EnvironmentFile=$ENV_FILE
Environment=ADMIN_API_LISTEN=127.0.0.1:8788
Environment=DOLPHIN_WEBROOT=$WEBROOT
Environment=DOLPHIN_HISTORY=/var/lib/dolphinclient/changelog-history
ExecStart=/usr/bin/node $OPT/admin-api.mjs
Restart=always
RestartSec=2
# It writes the published changelog and screenshots, which nginx serves as
# www-data — so it runs as www-data and may write nothing else on the disk.
User=www-data
Group=www-data
StateDirectory=dolphinclient
ReadWritePaths=$WEBROOT/downloads
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=yes
RestrictAddressFamilies=AF_INET AF_INET6
MemoryMax=192M

[Install]
WantedBy=multi-user.target
EOF
ok "dolphinclient-admin-api.service"

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
say "Installing the nginx configuration"
backup() { [[ -f "$1" ]] && cp -a "$1" "$1.bak-$(date +%Y%m%d%H%M%S)"; :; }
backup /etc/nginx/snippets/dolphinclient-locations.conf
install -m 0644 "$ROOT/deploy/nginx/dolphinclient-locations.conf" /etc/nginx/snippets/
install -m 0644 "$ROOT/deploy/nginx/dolphinclient-admin.conf" /etc/nginx/snippets/
install -m 0644 "$ROOT/deploy/nginx/dolphinclient-cf-geo.conf" /etc/nginx/conf.d/
ok "snippets/{dolphinclient-locations,dolphinclient-admin}.conf + conf.d/dolphinclient-cf-geo.conf"

install -d -m 0755 "$WEBROOT/admin-data"
chown www-data:www-data "$WEBROOT/admin-data" || true

# ---------------------------------------------------------------------------
say "Starting and checking"
systemctl daemon-reload
systemctl enable dolphinclient-access.service >/dev/null
systemctl enable dolphinclient-admin-api.service >/dev/null
systemctl enable dolphinclient-admin-stats.timer >/dev/null
# restart, not "enable --now": an already-running gate would keep serving with
# the OLD access.env, so a freshly entered team/AUD would silently do nothing.
systemctl restart dolphinclient-access.service
systemctl restart dolphinclient-admin-api.service
systemctl restart dolphinclient-admin-stats.timer
systemctl start dolphinclient-admin-stats.service || true
ok "Services running"

nginx -t
systemctl reload nginx
ok "nginx reloaded"

sleep 1
HEALTH="$(curl -fsS --max-time 5 http://127.0.0.1:8787/health || echo '{"configured":false}')"
echo "  Gate: $HEALTH"
echo "  API:  $(curl -fsS --max-time 5 http://127.0.0.1:8788/health || echo 'not answering')"

if grep -q '"configured":true' <<<"$HEALTH"; then
  printf '\n\033[32mDone.\033[0m The admin portal is reachable at https://example.invalid/admin —\n'
  printf 'only for identities your Cloudflare Access policy allows.\n'
else
  printf '\n\033[33mInstalled, but still SEALED (503).\033[0m\n'
  printf 'Create the Access application in the Cloudflare dashboard (deploy/ZERO-TRUST.md)\n'
  printf 'and then enter it here:\n\n'
  printf '  sudo deploy/setup-zero-trust.sh --team <your-team>.cloudflareaccess.com --aud <AUD tag>\n\n'
fi
