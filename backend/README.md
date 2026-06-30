# DolphinClient — Backend (`backend/`)

API für Accounts/Profile, Cosmetics-Besitz und den Launcher-Update-Feed.
**Node + TypeScript + Fastify**, später PostgreSQL + Objekt-Storage/CDN.

## Entwicklung

```bash
npm install                      # im Repo-Root (Workspaces)
cp backend/.env.example backend/.env
npm run dev --workspace backend  # http://localhost:3001/health
```

## Endpunkte (Skelett)

| Methode | Pfad | Zweck |
|---|---|---|
| GET | `/health` | Health-Check |
| GET | `/v1/profile/:uuid` | Profil + aktive Cosmetics + Einstellungen |
| GET | `/v1/cosmetics/:uuid` | Welche Cosmetics ein Spieler trägt (Client-Abruf) |
| GET | `/v1/updates/:channel` | Update-Manifest für den Launcher (signiert) |

## Hinweise

- **Keine Passwörter speichern** — Identität über Minecraft-Services-Token prüfen.
- Update-Manifeste **signieren**; der Launcher prüft die Signatur.
- Nutzergenerierte Capes brauchen **Moderation** (M5+).
