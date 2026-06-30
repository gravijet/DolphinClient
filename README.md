# DolphinClient

Ein performance-orientierter Minecraft-Client (Java Edition) im Stil großer
Clients wie Lunar Client — mit Fokus auf maximale FPS, eigenem Launcher,
Auto-Updates, Cosmetics, Website und einem erweiterbaren Mod-System.

> **Status:** Konzeptphase. Es gibt noch keinen Code, nur die Planung.
> Die vollständige Architektur, der Tech-Stack und die Roadmap stehen in
> [`docs/ROADMAP.md`](docs/ROADMAP.md).

## Die wichtigsten Punkte in einem Satz

- **Performance** kommt zu ~90 % aus bestehenden Open-Source-Mods
  (Sodium, Lithium, Iris, …). Der Mehrwert eines eigenen Clients liegt in
  **Bequemlichkeit, Cosmetics, Community und PvP-Features** — nicht darin,
  Sodium bei der reinen FPS-Zahl zu schlagen.
- **Eine Version zuerst.** Modern (Fabric) und 1.8.9 sind technisch fast zwei
  getrennte Projekte. Erst eine Version auf Top-Niveau bringen, dann erweitern.
- Die harten Hürden sind **nicht** die Technik, sondern **Recht (Mojang-EULA,
  Mod-Lizenzen), sichere Authentifizierung, Infrastruktur und Wartung**.

## Aufbau des Projekts (geplant)

| Komponente | Zweck | Tech (Vorschlag) |
|---|---|---|
| `client/`   | Der Minecraft-Mod selbst | Java, Fabric Loader, Mixin, Gradle/Loom |
| `launcher/` | Login, Spielstart, Auto-Update | Tauri **oder** Electron |
| `backend/`  | Accounts, Cosmetics, Update-Feed | Node/TS **oder** Go, PostgreSQL |
| `website/`  | Marketing, Download, Account, Store | Next.js |

Details, Begründungen und die phasenweise Roadmap: **[`docs/ROADMAP.md`](docs/ROADMAP.md)**.
