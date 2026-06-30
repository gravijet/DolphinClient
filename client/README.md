# DolphinClient — Mod (`client/`)

Der Minecraft-Mod selbst, als Fabric-Mod für **Minecraft 26.1**.

## Voraussetzungen

- **JDK 25** (26.1 erfordert Java 25 — der CI-Container hier hat nur 21,
  der Build erfolgt lokal mit JDK 25).
- Internet für Gradle/Loom/Fabric/Mojang-Maven beim ersten Build.

## Bauen

```bash
./gradlew build
# Ergebnis: build/libs/dolphinclient-<version>.jar
```

## Wichtige Hinweise

- **26.1 ist unobfuskiert** → Mojang Official Names, keine Mappings, kein
  Remapping. Loom 1.15 / Gradle 9.4.
- Die Minecraft-berührenden Klassen (`FpsModule`, `CoordsModule`, `HudManager`)
  verwenden offizielle Mojang-Namen (`Minecraft`, `GuiGraphics`, …). Diese
  **API-Berührungspunkte gegen die nun lesbare 26.1-Quelle verifizieren**,
  falls der Build meckert — die Architektur (Modul-System) bleibt davon
  unberührt.
- `fabric_version` in `gradle.properties` ist ein Platzhalter; exakten
  26.1-Build von Modrinth/CurseForge eintragen.

## Struktur

```
DolphinClient.java         ClientModInitializer (Einstiegspunkt, Keybinds)
module/Module.java         Basisklasse (an/aus, onTick, onRenderHud)
module/ModuleManager.java  Registry + ruft nur aktive Module auf
module/impl/*.java         FPS, Koordinaten, CPS, Keystrokes, Uhrzeit,
                           Sitzungszeit, Geschwindigkeit, Zoom
hud/HudContext.java        Auto-Layout fürs HUD (stapelt Zeilen)
hud/HudManager.java        Fabric-HUD-Callback -> Module
input/DolphinKeybindings   Tasten (Menü: Rechte Umschalt, Zoom: C)
gui/DolphinMenuScreen.java In-Game-Menü: Module an/aus
mixin/MouseHandlerMixin    zählt Linksklicks (CPS)
mixin/GameRendererMixin    skaliert das FOV (Zoom)
util/ClickTracker.java     rollierende Klickzählung (reine Logik)
config/DolphinConfig.java  JSON-Konfig (laden/speichern)
cosmetics/CosmeticsClient  Cape-Abruf vom Backend (Phase 3, Stub)
```

## Bedienung

- **Rechte Umschalt**: öffnet das DolphinClient-Menü (Module an/aus).
- **C halten**: Zoom (wenn das Zoom-Modul aktiviert ist).
- FPS ist standardmäßig an; alle weiteren Module (Koordinaten, CPS, Keystrokes,
  Uhrzeit, Sitzungszeit, Geschwindigkeit, Zoom) per Menü zuschaltbar.
