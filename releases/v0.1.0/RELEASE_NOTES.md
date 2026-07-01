# DolphinClient v0.1.0

Erste lauffähige Version. Performance-Client-Mod für **Minecraft 26.1** (Fabric),
plus Launcher-, Backend- und Website-Grundlage.

## Artefakte

| Datei | sha256 |
|---|---|
| `dolphinclient-0.1.0.jar` | `f91a5edc7ffcc278bca785c2c1b0381b6e2aa6236d7e1a0295dfd90233e32c2e` |
| `dolphinclient-0.1.0-sources.jar` | `bad44eecbfedf90f45d439a6fd11c04362eff8584417a971335c330bee55a774` |

Das Mod-Jar wurde gegen **echtes Minecraft 26.1** kompiliert (JDK 25,
Fabric Loom 1.17, Gradle 9.5.1, Loader 0.19.3, Fabric API 0.153.0+26.1.2).

## Installation (Client)

1. Fabric Loader **0.19.3** für 26.1 installieren.
2. **Fabric API** (0.153.0+26.1.2) in `.minecraft/mods/` legen.
3. `dolphinclient-0.1.0.jar` in `.minecraft/mods/` legen.
4. Mit Java 25 starten.

## Enthalten (Client v0.1)

- **In-Game-Menü** (Taste **Rechte Umschalt**) zum An-/Ausschalten der Module
- **Zoom** (Taste **C** halten)
- Performance-HUD mit zuschaltbaren Modulen (auch per Config
  `.minecraft/config/dolphinclient.json`):
  - FPS (Standard an)
  - Koordinaten + Blickrichtung
  - Uhrzeit
  - Sitzungszeit
  - Geschwindigkeit (Blöcke/s)
  - Keystrokes (WASD + LMB/RMB)

## Bekannte Einschränkungen

- **CPS** ist noch nicht enthalten: 26.1 bietet über `MouseHandler` keinen
  sauberen Klick-Hook mehr; wird nachgereicht.
- Das **In-Game-Aussehen** ist nicht visuell verifiziert (Build-Umgebung ohne
  Grafik) — der Code ist gegen die echte 26.1-API kompiliert.
- Launcher-Login (Microsoft) braucht eine Azure-`client_id`
  (`DOLPHIN_MS_CLIENT_ID`); Spielstart lädt Client-JAR + Libraries (Assets/
  Fabric-Merge folgen).
- Cosmetics: Backend + Datenschicht vorhanden; Cape-Rendering im Client folgt.
