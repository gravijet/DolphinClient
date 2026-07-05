# Deployment (`deploy/`)

Server-Skripte für `dolphin.gravijet.net` (Cloudflare → nginx-Origin auf dem
Host). Sie liegen auf dem Server unter `/opt/dolphinclient/` und werden als root
ausgeführt.

| Skript | Zweck |
|---|---|
| `redeploy.sh` | Website (Next.js static export) + Backend aus dem Repo-Checkout neu bauen, nach `/var/www/dolphin.gravijet.net` bzw. `/opt/dolphinclient/backend` veröffentlichen und die Dienste neu starten. Behält `downloads/` und `.well-known/`. |
| `publish-local.sh <version> <launcher-bin> [client-bin]` | **Lokal** gebaute Binaries unter den korrekten Asset-Namen in den Download-Ordner kopieren und `manifest.json` neu erzeugen. Ersetzt `update-downloads.sh`, seit nicht mehr auf GitHub gebaut wird. |
| `gen-manifest.mjs [dir] [version]` | `downloads/manifest.json` aus den vorhandenen nativen Binaries erzeugen (Größe + SHA-256). Wird von `publish-local.sh` aufgerufen. |
| ~~`update-downloads.sh [tag]`~~ | **Veraltet** — zog Binaries per `gh release download` aus dem GitHub-Release. Es wird nicht mehr auf GitHub gebaut; stattdessen `publish-local.sh` benutzen. |

> **Es wird nichts mehr auf GitHub gebaut.** Launcher und Client werden lokal
> gebaut — die komplette Build-Anleitung steht in
> [`../ANLEITUNG-BUILD.md`](../ANLEITUNG-BUILD.md).

## Ablauf für ein neues Release

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
