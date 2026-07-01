# DolphinClient — Mod (`client/`)

Der Minecraft-Mod selbst, als Fabric-Mod für **Minecraft 26.1**.
**v0.1: Performance-HUD** mit zuschaltbaren Modulen.

## Build (verifiziert)

- **JDK 25** erforderlich (26.1 braucht Java 25).
- Toolchain: **Fabric Loom 1.17-SNAPSHOT**, **Gradle 9.5.1**, Loader **0.19.3**,
  Fabric API **0.153.0+26.1.2**. 26.1 ist unobfuskiert (Mojang Official Names,
  keine Mappings).

```bash
./gradlew build
# Ergebnis: build/libs/dolphinclient-<version>.jar  (+ -sources.jar)
```

> Dieses Jar wurde gegen echtes Minecraft 26.1 kompiliert. Zum Spielen:
> Fabric Loader 0.19.3 + Fabric API für 26.1 installieren und das Jar in
> `.minecraft/mods/` legen.

## Bedienung

- **Rechte Umschalt**: öffnet das In-Game-Menü — Kopfzeile, „Alle an/aus"-
  Schnellschalter und pro Modul ein Button mit farbig hervorgehobenem Zustand.
- **C halten**: Zoom (wenn das Zoom-Modul aktiv ist).

## Module (v0.1)

HUD oben links, automatisch gestapelt. An/Aus über das Menü oder die Config
`.minecraft/config/dolphinclient.json` (FPS ist standardmäßig an):

| Modul | id | Default |
|---|---|---|
| FPS-Anzeige | `fps` | an |
| Koordinaten + Blickrichtung | `coords` | aus |
| Uhrzeit | `clock` | aus |
| Sitzungszeit | `session` | aus |
| Geschwindigkeit (b/s) | `speed` | aus |
| Keystrokes (WASD + LMB/RMB) | `keystrokes` | aus |

## Struktur

```
DolphinClient.java         ClientModInitializer (Einstiegspunkt)
module/Module.java         Basisklasse (an/aus, onTick, onRenderHud)
module/ModuleManager.java  Registry + ruft nur aktive Module auf
module/impl/*.java         FPS, Koordinaten, Uhrzeit, Sitzung, Speed, Keystrokes
hud/HudContext.java        Auto-Layout (GuiGraphicsExtractor.text/fill)
hud/HudManager.java        registriert HudElement (HudElementRegistry, 26.1)
config/DolphinConfig.java  JSON-Konfig (laden/speichern)
input/DolphinKeybindings   Tasten (Menü: Rechte Umschalt, Zoom: C)
gui/DolphinMenuScreen.java In-Game-Menü: Module an/aus
cosmetics/*                Cape-Abruf vom Backend (Phase 3, Datenschicht)
```

## 26.1-API-Hinweise (wichtig)

26.1 hat das Rendering stark umgebaut. Verifiziert gegen die echten 26.1-Klassen:

- Draw-Kontext ist **`GuiGraphicsExtractor`** (nicht mehr `GuiGraphics`);
  Text über `text(Font, String, x, y, color)`, Rechtecke über `fill(...)`.
- HUD wird über **`HudElementRegistry.addLast(Identifier, HudElement)`**
  registriert (statt `HudRenderCallback`).
- `Identifier.fromNamespaceAndPath(...)`, `Entity.position()` → `Vec3.x/y/z`.

Menü + Keybinds laufen über die neue 26.1-API: `KeyMappingHelper.registerKeyMapping`,
`KeyMapping.Category.register(Identifier)`, `Screen`/`Button.builder`. Zoom nutzt
`Options.fov()` (kein FOV-Render-Hook mehr).

### Zurückgestellt

**CPS**: 26.1 bietet über `MouseHandler` keinen sauberen Klick-Hook mehr —
wird nachgereicht, sobald ein tragfähiger Weg gefunden ist.
