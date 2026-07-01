# DolphinClient v0.1.0

Erste lauffähige Version. Performance-Client-Mod für **Minecraft 26.1** (Fabric),
plus Launcher-, Backend- und Website-Grundlage.

## Artefakte

| Datei | sha256 |
|---|---|
| `dolphinclient-0.1.0.jar` | `20b421fb5e2eccd0f46ebeaf5d70619837f9905018e869c51e93aae33e0547e9` |
| `dolphinclient-0.1.0-sources.jar` | `ecb0f1aedf12944bcaf58f9dcadbfcb725b6e9f080e8a760cbcdd39873257ae2` |

Das Mod-Jar wurde gegen **echtes Minecraft 26.1** kompiliert (JDK 25,
Fabric Loom 1.17, Gradle 9.5.1, Loader 0.19.3, Fabric API 0.153.0+26.1.2).

## Installation (Client)

1. Fabric Loader **0.19.3** für 26.1 installieren.
2. **Fabric API** (0.153.0+26.1.2) in `.minecraft/mods/` legen.
3. `dolphinclient-0.1.0.jar` in `.minecraft/mods/` legen.
4. Mit Java 25 starten.

## Enthalten (Client v0.1)

- **In-Game-Menü** (Taste **Rechte Umschalt**): Titel/Kopfzeile, „Alle an/aus"-
  Schnellschalter, pro Modul ein Button mit farbig hervorgehobenem Zustand
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
