# Performance-Mods — Lizenzprüfung (Bündeln)

Recherche-Stand: Juni 2026. **Vor jedem Release pro Mod und pro Version erneut
prüfen** — Lizenzen können sich zwischen Versionen ändern.

## Ergebnis

| Mod | Lizenz | Bündeln? | Bemerkung |
|---|---|---|---|
| **Sodium** (CaffeineMC) | LGPL-3.0 | ✅ mit Bedingungen | größter FPS-Hebel |
| **Lithium** (CaffeineMC) | LGPL-3.0 | ✅ mit Bedingungen | Tick-/Logik-Optimierung |
| **Iris** (IrisShaders) | LGPL-3.0 | ✅ mit Bedingungen | enthält `glsl-transformer` (**AGPL-3.0**) → Quelle bereitstellen |
| **ImmediatelyFast** (RaphiMC) | LGPL-3.0 | ✅ mit Bedingungen | Immediate-Mode-Rendering |
| **FerriteCore** (malte0811) | **MIT** | ✅ sehr permissiv | nur Lizenz + Copyright beilegen |
| **EntityCulling** (tr7zw) | **tr7zw Protective License** | ❌ **NICHT bündeln** | „Do not redistribute the JAR files anywhere else!" |
| **ModernFix** (embeddedt) | vermutlich LGPL-3.0 | 🟡 vor Nutzung prüfen | nicht endgültig verifiziert |

## Die wichtigste Empfehlung: nicht bündeln, sondern beim Install laden

Statt die Mod-JARs mitzuliefern (= Redistribution, löst alle LGPL/AGPL-Pflichten
aus und ist bei EntityCulling **verboten**), sollte der **Launcher die Mods zur
Installationszeit von der offiziellen Quelle laden** (z. B. Modrinth-API/-CDN).

Vorteile:
- **Keine Redistribution durch uns** → LGPL/AGPL-Pflichten und das
  EntityCulling-Verbot greifen gar nicht erst (der Nutzer lädt vom Original).
- Immer aktuelle, unveränderte, signierte Originale.
- Sauberer Update-Pfad.

Das ist der gleiche Ansatz, den seriöse Launcher fahren. **Klare Empfehlung für
DolphinClient.**

## Falls doch gebündelt wird (LGPL-3.0/MIT)

Nur für die LGPL-/MIT-Mods (EntityCulling bleibt außen vor), und nur unter
Einhaltung von:

1. **Als separate, unveränderte Jars** mitliefern (im `mods/`-Ordner) — **nicht**
   in den Client-Code shaden/mergen. (Minecraft-Mods erfüllen das natürlich.)
2. **Lizenztext + Copyright-Hinweise** jeder Mod beilegen.
3. **Quellcode bereitstellen** (Link auf das jeweilige Repo bzw. die genutzte
   Version genügt i. d. R.). Für Iris zusätzlich wegen `glsl-transformer` (AGPL).
4. **Ersetzbarkeit**: der Nutzer muss die Mod austauschen können (gegeben, da
   eigenständige Jars).
5. **Bei Änderungen** an einer LGPL-Mod: diese Änderungen unter LGPL offenlegen.
6. **Keine Endorsement-Suggestion / Markenrechte**: Namen nur sachlich nennen,
   nicht so tun, als seien die Autoren beteiligt/Partner.

## Konsequenzen für den Build-Plan

- **EntityCulling** aus der Standard-Bündelliste streichen. Optionen: per
  Launcher von Modrinth nachladen, um Erlaubnis fragen, oder eine
  permissiv lizenzierte Alternative wählen.
- Bevorzugter Weg insgesamt: **Mod-Manager im Launcher lädt das kuratierte
  Performance-Paket von Modrinth** statt Bündelung.
- Vor Release: kurze **anwaltliche Gegenprüfung** der LGPL/AGPL-Pflichten.

## Quellen

- Sodium / Lithium: LGPL-3.0 — CaffeineMC (github.com/CaffeineMC)
- Iris: LGPL-3.0 (+ glsl-transformer AGPL-3.0) — github.com/IrisShaders/Iris
- ImmediatelyFast: LGPL-3.0 — github.com/RaphiMC/ImmediatelyFast
- FerriteCore: MIT — github.com/malte0811/FerriteCore
- EntityCulling: tr7zw Protective License — github.com/tr7zw/EntityCulling
