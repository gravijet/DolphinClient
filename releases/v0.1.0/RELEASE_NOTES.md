# DolphinClient v0.1.0

Erste lauffähige Version. Performance-HUD-Mod für **Minecraft 26.1** (Fabric),
plus Launcher-, Backend- und Website-Grundlage.

## Artefakte

| Datei | sha256 |
|---|---|
| `dolphinclient-0.1.0.jar` | `7660d489ef164ad147d1f6c15edeb19c843a1aa181ac1f6e813b89413305416c` |
| `dolphinclient-0.1.0-sources.jar` | `410a90d80f03677e4b90b57cbba6c7569076d185a50d49504f070e0f309e5ee3` |

Das Mod-Jar wurde gegen **echtes Minecraft 26.1** kompiliert (JDK 25,
Fabric Loom 1.17, Gradle 9.5.1, Loader 0.19.3, Fabric API 0.153.0+26.1.2).

## Installation (Client)

1. Fabric Loader **0.19.3** für 26.1 installieren.
2. **Fabric API** (0.153.0+26.1.2) in `.minecraft/mods/` legen.
3. `dolphinclient-0.1.0.jar` in `.minecraft/mods/` legen.
4. Mit Java 25 starten.

## Enthalten (Client v0.1)

Performance-HUD mit zuschaltbaren Modulen (Config:
`.minecraft/config/dolphinclient.json`):

- FPS (Standard an)
- Koordinaten + Blickrichtung
- Uhrzeit
- Sitzungszeit
- Geschwindigkeit (Blöcke/s)
- Keystrokes (WASD + LMB/RMB)

## Bekannte Einschränkungen

- **In-Game-Menü, CPS, Zoom** sind in v0.1 nicht enthalten: die dafür
  genutzten 26.1-APIs (`MouseHandler.onPress`, `GameRenderer.getFov`,
  Fabric-Keybinding) existieren in 26.1 nicht mehr in der bisherigen Form und
  werden in einer Folgeversion neu angebunden. Module werden bis dahin über die
  Config-Datei an-/ausgeschaltet.
- Launcher-Login (Microsoft) braucht eine Azure-`client_id`
  (`DOLPHIN_MS_CLIENT_ID`); Spielstart lädt Client-JAR + Libraries (Assets/
  Fabric-Merge folgen).
- Cosmetics: Backend + Datenschicht vorhanden; Cape-Rendering im Client folgt.
