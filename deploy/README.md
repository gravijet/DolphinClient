# Deployment (`deploy/`)

Server-Skripte für `dolphin.gravijet.net` (Cloudflare → nginx-Origin auf dem
Host). Sie liegen auf dem Server unter `/opt/dolphinclient/` und werden als root
ausgeführt.

| Skript | Zweck |
|---|---|
| `redeploy.sh` | Website (Next.js static export) + Backend aus dem Repo-Checkout neu bauen, nach `/var/www/dolphin.gravijet.net` bzw. `/opt/dolphinclient/backend` veröffentlichen und die Dienste neu starten. Behält `downloads/` und `.well-known/`. |
| `update-downloads.sh [tag]` | Native Launcher-Binaries aus dem GitHub-Release (privates Repo, via `gh`/`GH_TOKEN`) in den Download-Ordner ziehen, alte Electron-Installer entfernen und `manifest.json` neu erzeugen. Standard-Tag: `v0.2.0`. |
| `gen-manifest.mjs [dir] [version]` | `downloads/manifest.json` aus den vorhandenen nativen Binaries erzeugen (Größe + SHA-256). Wird von `update-downloads.sh` aufgerufen. |

## Ablauf für ein neues Release

```bash
# 1. Tag pushen -> CI baut die nativen Binaries und hängt sie ans Release
git tag -a vX.Y.Z -m "…" && git push origin vX.Y.Z

# 2. Auf dem Server (als root):
/opt/dolphinclient/redeploy.sh                 # Website + Backend live
GH_TOKEN=… /opt/dolphinclient/update-downloads.sh vX.Y.Z   # Downloads live
```

Das Download-Manifest wird von nginx mit `Cache-Control: no-store` ausgeliefert,
neue Versionen erscheinen also sofort. Die Binaries selbst liegen unter
`/downloads/` (nginx setzt `Content-Disposition: attachment`).
