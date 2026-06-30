# DolphinClient

Ein performance-orientierter Minecraft-Client (Java Edition, **Ziel: 26.1**) im
Stil großer Clients wie Lunar Client — mit Fokus auf hohe FPS, eigenem
Launcher, Auto-Updates, Cosmetics, Website und einem erweiterbaren Mod-System.

> **Status:** Fundament-Phase (M0). Monorepo-Struktur und Skelette für alle
> vier Komponenten werden angelegt. Strategie & Architektur:
> [`docs/ROADMAP.md`](docs/ROADMAP.md). Konkreter Bauplan:
> [`docs/BUILD-PLAN.md`](docs/BUILD-PLAN.md).

## Die wichtigsten Punkte ehrlich

- **Performance** kommt zu ~90 % aus bestehenden Open-Source-Mods
  (Sodium, Lithium, Iris, …). Der Mehrwert eines eigenen Clients liegt in
  **Bequemlichkeit, Cosmetics, Community und PvP-Features** — nicht darin,
  Sodium bei der reinen FPS-Zahl zu schlagen.
- **26.1 ist unobfuskiert** → Modding ist deutlich einfacher als früher
  (keine Mappings). Braucht **Java 25**.
- Die harten Hürden sind **Recht (Mojang-EULA, Mod-Lizenzen), sichere
  Authentifizierung, Infrastruktur und Wartung** — nicht der Code selbst.

## Monorepo

| Ordner | Zweck | Tech | Status |
|---|---|---|---|
| `client/`   | Der Minecraft-Mod | Java 25, Fabric Loom 1.15, Gradle 9.4 | Skelett |
| `launcher/` | Login, Spielstart, Auto-Update | Electron + TypeScript | Skelett |
| `backend/`  | Accounts, Cosmetics, Update-Feed | Node/TS, Fastify | Skelett |
| `website/`  | Marketing, Download, Account | Next.js | Skelett |

Die JS/TS-Teile (`launcher`, `backend`, `website`) sind npm-Workspaces.

```bash
# JS-Abhängigkeiten installieren (launcher, backend, website)
npm install

# Client bauen (benötigt JDK 25!)
cd client && ./gradlew build
```

Details, Begründungen und die phasenweise Roadmap:
**[`docs/ROADMAP.md`](docs/ROADMAP.md)** · **[`docs/BUILD-PLAN.md`](docs/BUILD-PLAN.md)**
