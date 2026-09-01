# Deployment (`deploy/`)

Server-Skripte für `dolphinclient.de` (Cloudflare → nginx-Origin auf dem
Host). Sie liegen auf dem Server unter `/opt/dolphinclient/` und werden als root
ausgeführt.

| Skript | Zweck |
|---|---|
| `redeploy.sh` | Website (Next.js static export) + Backend aus dem Repo-Checkout neu bauen, nach `/var/www/dolphinclient.de` bzw. `/opt/dolphinclient/backend` veröffentlichen und die Dienste neu starten. Behält `downloads/` und `.well-known/`. |
| `publish-local.sh <version> <launcher-bin> [client-bin]` | Lokal gebaute Binaries unter dem anhand von OS + CPU ermittelten Asset-Namen veröffentlichen und `manifest.json` erneuern. |
| `gen-manifest.mjs [dir] [version]` | Architekturgenaues Manifest für Windows x64, Linux x64/ARM64 und macOS Intel/Apple Silicon erzeugen (Größe + SHA-256, mit Alt-Launcher-Aliassen). |
| `setup-zero-trust.sh` | **Admin-Portal** (`/admin`) einrichten: Origin-Gate + Statistik-Timer + nginx-Konfiguration. Anleitung: [`ZERO-TRUST.md`](ZERO-TRUST.md). |
| `access-gate.mjs` | Prüft das Cloudflare-Access-Token am Ursprung (`auth_request`, nur 127.0.0.1). Selbsttest: `node deploy/test-access-gate.mjs`. |
| `admin-stats.mjs` | Erzeugt `admin-data/stats.json` (Downloads, Besucher, Release, System) aus den nginx-Logs. |
| `nginx/` | Die Server-Konfiguration dieser Seite, versioniert. `setup-zero-trust.sh` installiert sie nach `/etc/nginx/`. |
| ~~`update-downloads.sh [tag]`~~ | **Veraltet** — zog Binaries per `gh release download` aus dem GitHub-Release. Es wird nicht mehr auf GitHub gebaut; stattdessen `publish-local.sh` benutzen. |

> **Es wird nichts mehr auf GitHub gebaut.** Launcher und Client werden lokal
> gebaut — die komplette Build-Anleitung steht in
> [`../ANLEITUNG-BUILD.md`](../ANLEITUNG-BUILD.md).

## Der einfachste Weg: `../release.sh`

Für ein komplettes Release gibt es im Repo-Wurzelverzeichnis **ein** Skript, das
alles Untenstehende automatisch macht (Version + Changelog abfragen, auf dem
Linux-Release-Host Windows + Linux bauen, auf einem Mac macOS bauen,
veröffentlichen, committen, pushen):

```bash
./release.sh            # fragt Version + Changelog, dann läuft alles allein
./release.sh --help     # u. a. --linux, --macos, --no-windows, --no-publish, -y
```

Es baut die Binaries **einmal** unprivilegiert und ruft `redeploy.sh` bzw.
`publish-windows.sh` danach mit `SKIP_BUILD=1` als root auf — so wird nicht
doppelt gebaut und root muss kein `cargo`/`npm` ausführen. Die manuellen
Schritte unten bleiben für Teil-Builds / Fehlersuche gültig.

## Ablauf für ein neues Release (von Hand)

```bash
# 1. Lokal bauen (siehe ANLEITUNG-BUILD.md):
cd launcher-native && cargo build --release && cd ..
cd client-rust     && cargo build --release && cd ..

# 2. Auf dem Server (als root):
/opt/dolphinclient/redeploy.sh                 # Website + Backend live
deploy/publish-local.sh X.Y.Z \                # Downloads live (dieses OS)
  launcher-native/target/release/dolphinclient-launcher \
  client-rust/target/release/dolphinclient
```

Das Download-Manifest wird von nginx mit `Cache-Control: no-store` ausgeliefert,
neue Versionen erscheinen also sofort. Die Binaries selbst liegen unter
`/downloads/` (nginx setzt `Content-Disposition: attachment`).
