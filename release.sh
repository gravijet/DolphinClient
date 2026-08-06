#!/usr/bin/env bash
# =============================================================================
#  DolphinClient — Ein-Klick-Release für Windows
# =============================================================================
#  Ein Skript, das ALLES macht: Version hochsetzen, Changelog AUTOMATISCH
#  erstellen, Client + Launcher (Windows) + Website bauen, auf dolphinclient.de
#  veröffentlichen und zu GitHub pushen.
#
#  Du musst nur DIESES Skript ausführen. Du schreibst KEINEN Changelog (er wird
#  vorgeschlagen — Enter genügt) und das Skript bricht bei einem Fehler NIE
#  einfach ab: es fragt, ob es den Schritt wiederholen, alles neu machen oder
#  (nur mit deiner Zustimmung) abbrechen soll.
#
#  Aufruf (im Repo-Wurzelverzeichnis):
#     ./release.sh
#
#  Optional:
#     ./release.sh 0.21.0                  # Version direkt vorgeben
#     ./release.sh -m "Titelzeile"         # Changelog-Überschrift vorgeben
#     ./release.sh --changelog punkte.txt  # Changelog-Punkte aus Datei (1/Zeile)
#     ./release.sh --linux                 # zusätzlich Linux bauen+veröffentlichen
#     ./release.sh --no-publish            # nur bauen+committen, nichts hochladen
#     ./release.sh --no-push               # veröffentlichen, aber nicht pushen
#     ./release.sh -y                      # Zusammenfassung ohne Rückfrage bestätigen
#
#  Voraussetzungen (einmalig, siehe ANLEITUNG-BUILD.md): rust (stable+nightly)
#  mit x86_64-pc-windows-gnu-Target, gcc/g++-mingw-w64, makensis, node, sudo.
# =============================================================================
# Kein „set -e": Fehler werden bewusst selbst behandelt (nie hartes Abbrechen).
set -o pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"
SCRIPT_START=$SECONDS
TIMING_FILE="$ROOT/.release-timings"      # gemerkte Dauer je Task (gitignored)

# ---- Farben / Ausgabe ------------------------------------------------------
if [[ -t 1 ]]; then
  B=$'\e[1m'; DIM=$'\e[2m'; GRN=$'\e[32m'; YLW=$'\e[33m'; RED=$'\e[31m'; CYN=$'\e[36m'; R=$'\e[0m'
else
  B=""; DIM=""; GRN=""; YLW=""; RED=""; CYN=""; R=""
fi
say()  { printf '%s\n' "$*"; }
step() { printf '\n%s▸ %s%s\n' "$B$CYN" "$*" "$R"; }
ok()   { printf '  %s✓%s %s\n' "$GRN" "$R" "$*"; }
warn() { printf '  %s!%s %s\n' "$YLW" "$R" "$*"; }

# ---- erwartete Dauern (Sekunden), werden nach jedem Lauf verfeinert --------
declare -A DEF=(
  [launcher_win]=240  [client_win]=600  [website_build]=70
  [publish_win]=70    [publish_website]=40
  [launcher_linux]=200 [client_linux]=520 [publish_linux]=20
)

# =============================================================================
#  Argumente
# =============================================================================
VERSION=""; HEADLINE=""; ITEMS_FILE=""; AUTO_ITEMS=""
DO_LINUX=0; DO_PUBLISH=1; DO_PUSH=1; ASSUME_YES=0; DO_WEBSITE="auto"
while [[ $# -gt 0 ]]; do
  case "$1" in
    -m|--message)  HEADLINE="${2:-}"; shift 2 ;;
    --changelog)   ITEMS_FILE="${2:-}"; shift 2 ;;
    --linux)       DO_LINUX=1; shift ;;
    --website)     DO_WEBSITE=1; shift ;;
    --no-website)  DO_WEBSITE=0; shift ;;
    --no-publish)  DO_PUBLISH=0; DO_PUSH=0; shift ;;
    --no-push)     DO_PUSH=0; shift ;;
    -y|--yes)      ASSUME_YES=1; shift ;;
    -h|--help)     awk 'NR>1 && /^#/{sub(/^# ?/,""); print; next} NR>1{exit}' "$0"; exit 0 ;;
    -*)            warn "Unbekannte Option ignoriert: $1"; shift ;;
    *)             [[ -z "$VERSION" ]] && VERSION="$1"; shift ;;
  esac
done

# Website neu bauen? Der Changelog wird zur Laufzeit aus downloads/changelog.json
# geladen (wie das Manifest), also braucht ein reines Client-Release KEINEN
# Next.js-Neubau mehr. Wir bauen die Website nur, wenn sich unter website/ etwas
# ANDERES als die laufzeit-veröffentlichte changelog.json geändert hat (oder der
# vorhandene Export fehlt). --website erzwingt den Neubau, --no-website überspringt.
website_source_changed() {
  git -C "$ROOT" status --porcelain -- website/ 2>/dev/null \
    | sed 's/^...//' \
    | grep -vFx 'website/app/changelog/changelog.json' \
    | grep -q .
}
decide_website() {
  case "$DO_WEBSITE" in
    1|0) return ;;
  esac
  if [[ ! -d "$ROOT/website/out" ]]; then DO_WEBSITE=1
  elif website_source_changed;      then DO_WEBSITE=1
  else                                   DO_WEBSITE=0
  fi
}
trap '[[ -n "$AUTO_ITEMS" && -f "$ITEMS_FILE" ]] && rm -f "$ITEMS_FILE"' EXIT

# =============================================================================
#  Hilfsfunktionen: Zeit, Balken, Timings
# =============================================================================
fmt() { local s=${1:-0}; ((s<0)) && s=0; printf '%d:%02d' $((s/60)) $((s%60)); }

bar() { # $1=prozent $2=breite
  local p=${1:-0} w=${2:-24} f i out=""
  ((p<0)) && p=0; ((p>100)) && p=100
  f=$(( p*w/100 ))
  for ((i=0;i<w;i++)); do ((i<f)) && out+="█" || out+="░"; done
  printf '%s' "$out"
}

get_expected() { # $1=key $2=default
  local v; v=$(grep -E "^$1=" "$TIMING_FILE" 2>/dev/null | tail -1 | cut -d= -f2)
  [[ "$v" =~ ^[0-9]+$ && "$v" -gt 0 ]] && printf '%s' "$v" || printf '%s' "$2"
}
save_timing() { # $1=key $2=sekunden
  local tmp; tmp=$(mktemp)
  grep -vE "^$1=" "$TIMING_FILE" 2>/dev/null >"$tmp"
  printf '%s=%s\n' "$1" "$2" >>"$tmp"
  mv "$tmp" "$TIMING_FILE" 2>/dev/null || true
}

# =============================================================================
#  Fortschrittsanzeige (zwei Balken: Gesamt + aktueller Task)
# =============================================================================
OVERALL_TOTAL=1; COMPLETED_EXPECTED=0; PROG_ACTIVE=0

prog_draw() { # $1=label $2=task% $3=task_el $4=task_exp $5=detail
  local tl="$1" tp="$2" te="$3" tx="$4" det="$5"
  local capped=$(( te<tx ? te : tx ))
  local od=$(( COMPLETED_EXPECTED + capped ))
  local op=$(( OVERALL_TOTAL>0 ? od*100/OVERALL_TOTAL : 0 )); ((op>99)) && op=99
  local wall=$(( SECONDS - SCRIPT_START ))
  ((PROG_ACTIVE)) && printf '\e[2A'
  printf '\r\e[K  %sGesamt%s          [%s] %3d%%  %s / ~%s\n' \
    "$B$CYN" "$R" "$(bar "$op" 24)" "$op" "$(fmt "$wall")" "$(fmt "$OVERALL_TOTAL")"
  printf '\r\e[K  %-16.16s [%s] %3d%%  %s / ~%s  %s%s%s\n' \
    "$tl" "$(bar "$tp" 24)" "$tp" "$(fmt "$te")" "$(fmt "$tx")" "$DIM" "$det" "$R"
  PROG_ACTIVE=1
}
prog_collapse() { # $1=symbol $2=farbe $3=label $4=el
  ((PROG_ACTIVE)) && printf '\e[2A\r\e[K'
  printf '  %s%s%s %-16s %s(%s)%s\n' "$2" "$1" "$R" "$3" "$DIM" "$(fmt "$4")" "$R"
  printf '\r\e[K'
  PROG_ACTIVE=0
}

detail_from_log() { # $1=typ $2=logdatei
  local line=""
  case "$1" in
    cargo)
      line=$(grep -oE 'Compiling [A-Za-z0-9_.+-]+' "$2" 2>/dev/null | tail -1)
      if [[ -n "$line" ]]; then
        local n; n=$(grep -c 'Compiling ' "$2" 2>/dev/null); line="$line (#${n:-0})"
      fi ;;
    web)
      line=$(grep -iE 'compiled|Creating|Generating|Collecting|Route|Linting|Compiling' "$2" 2>/dev/null | tail -1) ;;
  esac
  [[ -z "$line" ]] && line=$(grep -v '^[[:space:]]*$' "$2" 2>/dev/null | tail -1)
  line=${line//$'\t'/ }; line=${line//$'\r'/}
  printf '%s' "${line:0:40}"
}

LAST_LOG=""
run_task() { # $1=key $2=label $3=typ ; danach Kommando(=Funktion)
  local key="$1" label="$2" typ="$3"; shift 3
  local exp; exp=$(get_expected "$key" "${DEF[$key]:-120}")
  local log; log=$(mktemp); LAST_LOG="$log"
  local start=$SECONDS
  ( "$@" ) >"$log" 2>&1 &
  local pid=$!
  while kill -0 "$pid" 2>/dev/null; do
    local el=$(( SECONDS - start ))
    local p=$(( exp>0 ? el*100/exp : 0 )); ((p>95)) && p=95
    prog_draw "$label" "$p" "$el" "$exp" "$(detail_from_log "$typ" "$log")"
    sleep 0.5
  done
  wait "$pid"; local rc=$?
  local el=$(( SECONDS - start ))
  if ((rc==0)); then
    save_timing "$key" "$el"
    COMPLETED_EXPECTED=$(( COMPLETED_EXPECTED + exp ))
    prog_collapse "✓" "$GRN" "$label" "$el"
    rm -f "$log"; LAST_LOG=""
  else
    prog_collapse "✗" "$RED" "$label" "$el"
  fi
  return $rc
}

# =============================================================================
#  Schritt-Ausführung mit Wiederholung (bricht NIE ungefragt ab)
# =============================================================================
#   Rückgabe: 0 = ok,  2 = „alles von vorne".  Bei „abbrechen" wird nach
#   ausdrücklicher Bestätigung mit exit beendet.
do_step() { # $1=titel ; danach Funktion(+Argumente)
  local title="$1"; shift
  while true; do
    LAST_LOG=""
    if "$@"; then return 0; fi
    printf '\n%s✗ Schritt fehlgeschlagen: %s%s\n' "$B$RED" "$title" "$R"
    if [[ -n "$LAST_LOG" && -f "$LAST_LOG" ]]; then
      printf '%s  ── letzte Ausgabe ─────────────────────────────%s\n' "$DIM" "$R"
      tail -n 20 "$LAST_LOG" | sed 's/^/  /'
      printf '%s  ───────────────────────────────────────────────%s\n' "$DIM" "$R"
    fi
    printf '  Was tun?  [%sEnter%s] Schritt wiederholen   [%sa%s] alles von vorne   [%sx%s] abbrechen\n' \
      "$B" "$R" "$B" "$R" "$B" "$R"
    local c; read -r -p "  > " c </dev/tty || c=""
    case "${c,,}" in
      a|alles) return 2 ;;
      x|abbrechen)
        local y; read -r -p "  Wirklich abbrechen? Es wurde noch nichts veröffentlicht. [j/N] " y </dev/tty || y=""
        [[ "${y,,}" == j* || "${y,,}" == y* ]] && { printf '%sAbgebrochen.%s\n' "$YLW" "$R"; exit 1; } ;;
      *) : ;;  # wiederholen
    esac
  done
}

# =============================================================================
#  Eingaben (Version + Changelog) — validieren in Schleife, nie abbrechen
# =============================================================================
CUR="0.0.0"; SUGGEST="0.1.0"
collect_version() {
  CUR="$(node -p "require('./package.json').version" 2>/dev/null || echo 0.0.0)"
  local a b; IFS='.' read -r a b _ <<<"$CUR"; SUGGEST="${a:-0}.$(( ${b:-0} + 1 )).0"
  if [[ -n "$VERSION" && "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then return 0; fi
  [[ -n "$VERSION" ]] && warn "Ungültige Version '$VERSION' — bitte neu eingeben."
  VERSION=""
  while [[ -z "$VERSION" ]]; do
    step "Version"
    say "  Aktuell: ${B}${CUR}${R}"
    read -r -p "  Neue Version [${SUGGEST}]: " VERSION </dev/tty || VERSION=""
    VERSION="${VERSION:-$SUGGEST}"
    [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { warn "Ungültig (Format X.Y.Z)."; VERSION=""; }
  done
}

manual_changelog() {
  step "Changelog selbst schreiben"
  local h; read -r -p "  Überschrift: " h </dev/tty || h=""
  [[ -n "$h" ]] && HEADLINE="$h"
  say "  Punkte — einer pro Zeile, leere Zeile beendet die Eingabe:"
  : >"$ITEMS_FILE"; AUTO_ITEMS=1
  while true; do
    local l; read -r -p "    • " l </dev/tty || break
    [[ -z "$l" ]] && break
    printf '%s\n' "$l" >>"$ITEMS_FILE"
  done
  [[ -s "$ITEMS_FILE" ]] || printf 'Kleinere Verbesserungen und Fehlerbehebungen\n' >"$ITEMS_FILE"
}

collect_changelog() {
  # Von der Kommandozeile vorgegeben? Dann direkt übernehmen.
  if [[ -n "$HEADLINE" && -n "$ITEMS_FILE" && -f "$ITEMS_FILE" ]]; then return 0; fi

  # Automatisch vorschlagen — du musst nichts schreiben.
  local draft; draft=$(mktemp)
  node "$ROOT/deploy/suggest-changelog.mjs" >"$draft" 2>/dev/null || true
  local auto_head; auto_head=$(head -1 "$draft")
  [[ -n "$HEADLINE" ]] || HEADLINE="${auto_head:-Wartung & Verbesserungen}"
  ITEMS_FILE=$(mktemp); AUTO_ITEMS=1
  tail -n +2 "$draft" >"$ITEMS_FILE"
  [[ -s "$ITEMS_FILE" ]] || printf 'Kleinere Verbesserungen und Fehlerbehebungen\n' >"$ITEMS_FILE"
  rm -f "$draft"

  step "Changelog — automatisch erstellt (du musst nichts schreiben)"
  say "  ${B}${HEADLINE}${R}"
  sed 's/^/    • /' "$ITEMS_FILE"
  say ""
  say "  [${B}Enter${R}] übernehmen    [${B}b${R}] lieber selbst schreiben"
  local c; read -r -p "  > " c </dev/tty || c=""
  case "${c,,}" in
    b|bearbeiten|selbst) manual_changelog ;;
    *) : ;;
  esac
}

# =============================================================================
#  Zusammenfassung + Bestätigung
# =============================================================================
COMMIT_MSG=""
confirm_summary() {
  while true; do
    COMMIT_MSG="${VERSION}: ${HEADLINE}"
    local nitems; nitems=$(grep -cve '^[[:space:]]*$' "$ITEMS_FILE" 2>/dev/null || echo 0)
    step "Zusammenfassung"
    printf '  Version:         %s  ->  %s%s%s\n' "$CUR" "$B" "$VERSION" "$R"
    printf '  Changelog:       „%s" (%s Punkte)\n' "$HEADLINE" "$nitems"
    printf '  Bauen:           Windows-Launcher + Windows-Client%s%s\n' \
      "$( ((DO_WEBSITE)) && echo ' + Website' || echo " ${DIM}(Website unverändert — übersprungen)${R}")" \
      "$( ((DO_LINUX)) && echo ' + Linux')"
    printf '  Veröffentlichen: %s\n' "$( ((DO_PUBLISH)) && echo 'Downloads + Website live auf dolphinclient.de' || echo "${YLW}nein (--no-publish)${R}")"
    printf '  Git:             %s\n' "$( ((DO_PUSH)) && echo "commit + push  \"$COMMIT_MSG\"" || echo "${YLW}nur lokal committen (kein Push)${R}")"
    say ""
    say "  ${DIM}Wird committet (git add -A):${R}"
    git status --short | head -n 24 | sed 's/^/    /'
    [[ "$(git status --porcelain | wc -l)" -gt 24 ]] && say "    …"
    ((ASSUME_YES)) && return 0
    say ""
    say "  [${B}j${R}] los geht's    [${B}e${R}] Eingaben ändern    [${B}n${R}] doch nicht"
    local c; read -r -p "  > " c </dev/tty || c=""
    case "${c,,}" in
      j|ja|y|yes) return 0 ;;
      e|eingaben|ändern) VERSION=""; HEADLINE=""; [[ -n "$AUTO_ITEMS" ]] && ITEMS_FILE=""; collect_version; collect_changelog ;;
      n|nein|no) say "  Nichts geändert. Tschüss."; exit 0 ;;
      *) : ;;
    esac
  done
}

# =============================================================================
#  Einzelne Schritte
# =============================================================================
step_preflight() {
  step "Werkzeuge prüfen"
  local miss=() t
  for t in cargo rustup node npm makensis x86_64-w64-mingw32-gcc sudo git; do
    command -v "$t" >/dev/null 2>&1 || miss+=("$t")
  done
  rustup target list --installed 2>/dev/null | grep -q '^x86_64-pc-windows-gnu$' \
    || miss+=("rust-target x86_64-pc-windows-gnu")
  rustup target list --toolchain nightly --installed 2>/dev/null | grep -q '^x86_64-pc-windows-gnu$' \
    || miss+=("nightly-target x86_64-pc-windows-gnu")
  if ((${#miss[@]})); then
    warn "Fehlt: ${miss[*]}"
    say "  Installiere die fehlenden Teile (siehe ANLEITUNG-BUILD.md) und wiederhole."
    return 1
  fi
  ok "Alle Werkzeuge vorhanden."
  return 0
}

step_fix_perms() {
  step "Dateirechte in Ordnung bringen (verhindert Build-Fehler)"
  local me grp; me=$(id -un); grp=$(id -gn)
  # Frühere sudo-/root-Läufe können target/, node_modules/ & Co. root gehören
  # lassen — dann darf cargo/npm nicht mehr schreiben. Alles außer .git dem
  # aktuellen Benutzer zurückgeben.
  sudo find "$ROOT" -mindepth 1 -maxdepth 1 ! -name .git -exec chown -R "$me:$grp" {} + 2>/dev/null
  ok "Rechte gehören wieder $me."
  return 0
}

step_bump() {
  step "Version auf $VERSION setzen"
  node -e '
    const fs=require("fs"); const v=process.argv[1];
    for (const f of ["package.json","package-lock.json"]) {
      const j=JSON.parse(fs.readFileSync(f,"utf8"));
      j.version=v; if (j.packages && j.packages[""]) j.packages[""].version=v;
      fs.writeFileSync(f, JSON.stringify(j,null,2)+"\n");
    }
  ' "$VERSION" || return 1
  sed -i '0,/^version = ".*"/s//version = "'"$VERSION"'"/' launcher-native/Cargo.toml || return 1
  sed -i '0,/^version = ".*"/s//version = "'"$VERSION"'"/' client-rust/Cargo.toml     || return 1
  ok "package.json, package-lock.json, beide Cargo.toml"
  return 0
}

step_changelog() {
  step "Changelog eintragen"
  FORCE=1 node deploy/add-changelog.mjs "$VERSION" "$HEADLINE" "$ITEMS_FILE" | sed 's/^/  /' \
    || return 1
  return 0
}

# --- Build-Kommandos (laufen im Hintergrund, Ausgabe geht ins Log) ----------
_bl_win() { cd "$ROOT/launcher-native" && cargo build --release --target x86_64-pc-windows-gnu; }
_bc_win() { cd "$ROOT/client-rust"     && cargo build --release --target x86_64-pc-windows-gnu; }
_bl_lin() { cd "$ROOT/launcher-native" && cargo build --release; }
_bc_lin() { cd "$ROOT/client-rust"     && cargo build --release; }
_web()    { cd "$ROOT" && npm install -w website --no-audit --no-fund \
              && cd "$ROOT/website" && npx next build; }
_pub_win() { sudo env SKIP_BUILD=1 "$ROOT/deploy/publish-windows.sh" "$VERSION"; }
_pub_web() { sudo env SKIP_BUILD=1 "$ROOT/deploy/redeploy.sh"; }
_pub_lin() { sudo "$ROOT/deploy/publish-local.sh" "$VERSION" \
              "$ROOT/launcher-native/target/release/dolphinclient-launcher" \
              "$ROOT/client-rust/target/release/dolphinclient"; }

step_build_launcher_win() { run_task launcher_win "Windows-Launcher" cargo _bl_win; }
step_build_client_win()   { run_task client_win   "Windows-Client"   cargo _bc_win; }
step_build_launcher_lin() { run_task launcher_linux "Linux-Launcher"  cargo _bl_lin; }
step_build_client_lin()   { run_task client_linux   "Linux-Client"    cargo _bc_lin; }
step_website()            { run_task website_build "Website" web _web && [[ -d "$ROOT/website/out" ]]; }
step_pub_win()            { run_task publish_win "Downloads (Win)" plain _pub_win; }
step_pub_web()            { run_task publish_website "Website live" plain _pub_web; }
step_pub_lin()            { run_task publish_linux "Downloads (Linux)" plain _pub_lin; }

step_commit() {
  step "Änderungen committen"
  git add -A || return 1
  if git diff --cached --quiet; then warn "Nichts zu committen."; return 0; fi
  git commit -q -m "$COMMIT_MSG" || return 1
  ok "Commit: $COMMIT_MSG"
  return 0
}

step_push() {
  step "Zu GitHub pushen"
  # Anmeldedaten liegen unter dem Benutzer benj (nicht claude-runner).
  if [[ "$(id -un)" == "benj" ]]; then
    git push origin HEAD:main || return 1
  else
    sudo -u benj git -C "$ROOT" push origin HEAD:main || return 1
  fi
  ok "Gepusht nach origin/main."
  return 0
}

# =============================================================================
#  Gesamtdauer + Ablauf
# =============================================================================
compute_total() {
  local keys=(launcher_win client_win)
  ((DO_LINUX)) && keys+=(launcher_linux client_linux)
  ((DO_WEBSITE)) && keys+=(website_build)
  if ((DO_PUBLISH)); then
    keys+=(publish_win); ((DO_LINUX)) && keys+=(publish_linux)
    ((DO_WEBSITE)) && keys+=(publish_website)
  fi
  OVERALL_TOTAL=0
  local k
  for k in "${keys[@]}"; do
    OVERALL_TOTAL=$(( OVERALL_TOTAL + $(get_expected "$k" "${DEF[$k]:-120}") ))
  done
  ((OVERALL_TOTAL>0)) || OVERALL_TOTAL=1
}

run_pipeline() {
  COMPLETED_EXPECTED=0; PROG_ACTIVE=0
  do_step "Dateirechte"        step_fix_perms            || return $?
  do_step "Version setzen"     step_bump                 || return $?
  do_step "Changelog"          step_changelog            || return $?

  # Cross-Compile-Umgebung laden (Linker etc.) und dann still bauen.
  # shellcheck source=/dev/null
  source "$ROOT/deploy/win-cross-env.sh"
  printf '\n%s▸ Bauen%s  %s(Ausgaben werden gebündelt — es erscheint nur der Fortschritt und das Ergebnis)%s\n' \
    "$B$CYN" "$R" "$DIM" "$R"

  do_step "Windows-Launcher bauen" step_build_launcher_win || return $?
  do_step "Windows-Client bauen"   step_build_client_win   || return $?
  if ((DO_LINUX)); then
    do_step "Linux-Launcher bauen" step_build_launcher_lin || return $?
    do_step "Linux-Client bauen"   step_build_client_lin   || return $?
  fi
  if ((DO_WEBSITE)); then
    do_step "Website bauen"        step_website            || return $?
  else
    printf '  %s⤳%s %-16s %s(unverändert — Neubau übersprungen; Changelog kommt via downloads/changelog.json)%s\n' \
      "$DIM" "$R" "Website" "$DIM" "$R"
  fi

  if ((DO_PUBLISH)); then
    printf '\n%s▸ Veröffentlichen%s\n' "$B$CYN" "$R"
    do_step "Downloads veröffentlichen" step_pub_win       || return $?
    ((DO_LINUX)) && { do_step "Linux-Downloads" step_pub_lin || return $?; }
    if ((DO_WEBSITE)); then
      do_step "Website live schalten"   step_pub_web        || return $?
    fi
  fi

  do_step "Committen" step_commit || return $?
  ((DO_PUSH)) && { do_step "Pushen" step_push || return $?; }
  return 0
}

# =============================================================================
#  Hauptprogramm
# =============================================================================
main() {
  printf '%s╔═══════════════════════════════════════════════╗%s\n' "$B$CYN" "$R"
  printf '%s║   DolphinClient — Ein-Klick-Release (Windows) ║%s\n' "$B$CYN" "$R"
  printf '%s╚═══════════════════════════════════════════════╝%s\n' "$B$CYN" "$R"

  while true; do do_step "Werkzeuge prüfen" step_preflight; (($?==2)) && continue; break; done
  collect_version
  collect_changelog
  decide_website
  confirm_summary
  compute_total

  local rc
  while true; do
    run_pipeline; rc=$?
    if ((rc==2)); then printf '\n%s↻ Von vorne …%s\n' "$YLW" "$R"; continue; fi
    break
  done

  local total=$(( SECONDS - SCRIPT_START ))
  printf '\n%s✓ Release %s fertig%s  %s(Gesamtdauer %s)%s\n' "$B$GRN" "$VERSION" "$R" "$DIM" "$(fmt "$total")" "$R"
  if ((DO_PUBLISH)); then
    say "  • Download-Seite:  ${CYN}https://dolphinclient.de/download${R}"
    say "  • Manifest:        ${CYN}https://dolphinclient.de/downloads/manifest.json${R}"
    say "  • Changelog:       ${CYN}https://dolphinclient.de/changelog${R}"
  fi
}

# Nur ausführen, wenn direkt gestartet (nicht beim Sourcen zum Testen).
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  main
fi
