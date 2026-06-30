# DolphinClient — Großer Umsetzungsplan (Build-Plan)

Konkreter, ausführbarer Bauplan für DolphinClient auf **Minecraft 26.1**.
Dies ist die Detailebene unter [`ROADMAP.md`](ROADMAP.md): Monorepo-Struktur,
Tech-Entscheidungen mit exakten Versionen, Aufgaben pro Komponente,
Meilensteine und „Definition of Done".

> **Realismus vorab:** Ein Client auf Lunar-Niveau ist Mann-Jahre Arbeit.
> Dieser Plan ist so geschnitten, dass **nach jeder Phase etwas Lauffähiges**
> existiert. Wir bauen das Fundament jetzt vollständig auf und füllen es dann
> Phase für Phase mit Funktion.

---

## 0. Bestätigte Eckdaten (26.1)

| Thema | Wert |
|---|---|
| Minecraft | **26.1** („Tiny Takeover", 24.03.2026), Schema `Jahr.Drop.Hotfix` |
| Besonderheit | **Vollständig unobfuskiert** → keine Mappings, kein Remapping |
| Java | **25** (zwingend) |
| RAM-Default | 4 GB |
| Mod-Loader | Fabric Loader **0.18.4** |
| Build-Tool | Fabric Loom **1.15**, Gradle **9.4** |
| Mappings | Mojang Official (Yarn abgekündigt) |
| Loom-Workflow | `implementation` statt `modImplementation`, kein `remapJar`-Zwang |

Alle Versionen sind in `client/gradle.properties` zentralisiert → Bump auf
26.2+ ist eine Zeile.

---

## 1. Monorepo-Struktur

```
DolphinClient/
├── README.md
├── .gitignore
├── package.json                # npm-Workspaces für launcher/backend/website
├── docs/
│   ├── ROADMAP.md              # Strategie, Architektur, Recht (Übersicht)
│   └── BUILD-PLAN.md           # dieses Dokument
│
├── client/                     # Der Minecraft-Mod (Java, Fabric, Gradle)
│   ├── build.gradle
│   ├── settings.gradle
│   ├── gradle.properties       # alle Versionen zentral
│   ├── gradlew / gradlew.bat   # Gradle-Wrapper (9.4)
│   └── src/main/
│       ├── java/com/dolphinclient/
│       │   ├── DolphinClient.java          # ClientModInitializer
│       │   ├── module/                     # Modul-System (an/aus)
│       │   │   ├── Module.java
│       │   │   ├── ModuleManager.java
│       │   │   └── impl/{FpsModule,CoordsModule,CpsModule}.java
│       │   ├── hud/HudManager.java         # HUD-Rendering
│       │   ├── config/DolphinConfig.java   # JSON-Konfig (laden/speichern)
│       │   └── cosmetics/CosmeticsClient.java  # holt Capes vom Backend
│       └── resources/
│           ├── fabric.mod.json
│           └── dolphinclient.mixins.json
│
├── launcher/                   # Desktop-Launcher (Electron + TypeScript)
│   ├── package.json
│   ├── tsconfig.json
│   └── src/
│       ├── main/
│       │   ├── main.ts                 # Electron-Hauptprozess
│       │   ├── auth/microsoft.ts       # MS OAuth (Device-Code-Flow)
│       │   ├── game/launch.ts          # Mojang-Dateien laden + JVM starten
│       │   └── updater.ts              # Auto-Update (signiert)
│       └── renderer/
│           ├── index.html
│           └── renderer.ts
│
├── backend/                    # API (Node + TypeScript + Fastify)
│   ├── package.json
│   ├── tsconfig.json
│   └── src/
│       ├── server.ts
│       └── routes/{profile,cosmetics,updates}.ts
│
└── website/                    # Marketing + Account (Next.js)
    ├── package.json
    ├── next.config.js
    └── app/{page,download/page,account/page}.tsx
```

---

## 2. Komponente: `client/` (der Mod)

**Tech:** Java 25, Fabric Loom 1.15, Gradle 9.4, Mojang Mappings, Mixin.

**Aufgaben Phase 1:**
1. ✅ Gradle-/Loom-Setup mit zentralen Versionen in `gradle.properties`.
2. ✅ `fabric.mod.json` + Mixin-Konfig + Entrypoint (`DolphinClient`).
3. ✅ **Modul-System**: Basisklasse `Module` (Name, `enabled`, `onTick`,
   `onRender`) + `ModuleManager` (Registry, an/aus, Persistenz).
   → Erfüllt den Wunsch „nur aktiv = nur dann Leistung": deaktivierte
   Module durchlaufen keinen Code-Pfad.
4. ✅ **HUD-Module**: FPS, Koordinaten, CPS (Beispiele).
5. ✅ **Config**: JSON in `.minecraft/config/dolphinclient.json`.
6. ⬜ Performance-Mods bündeln (Sodium, Lithium, FerriteCore, EntityCulling) —
   **erst nach Lizenzprüfung** (siehe ROADMAP §8). Vorerst nur Hinweis/Stub.
7. ⬜ In-Game-Konfig-GUI (Cloth Config).
8. ⬜ Cosmetics-Renderer (Capes) — Client-Teil von Phase 3.

**Definition of Done (Phase 1):** Mod kompiliert mit Java 25, lädt in 26.1,
zeigt FPS-HUD, Module per Konfig an/abschaltbar.

> ⚠️ **Hinweis zum Container:** Hier läuft Java 21; 26.1 braucht Java 25. Das
> Skelett ist korrekt für 26.1 eingerichtet, der finale Build erfolgt lokal
> mit JDK 25.

---

## 3. Komponente: `launcher/`

**Tech:** Electron + TypeScript. Auto-Update via `electron-updater`.

**Aufgaben Phase 2:**
1. ✅ Electron-Grundgerüst (Haupt-/Renderer-Prozess, Build-Skripte).
2. ⬜ **Microsoft-OAuth** (Device-Code-Flow) → Xbox Live → Minecraft-Services.
   Tokens in OS-Keychain. **Nur legitimer Login, keine Cracked-Accounts.**
3. ⬜ **Spielstart**: Version-Manifest von Mojang lesen, Libraries/Assets für
   26.1 laden (von Mojang, nie selbst gehostet), Classpath bauen, JDK 25
   starten, DolphinClient-Mod + Fabric injizieren.
4. ⬜ **Auto-Update**: signiertes Manifest vom Backend, Signaturprüfung.
5. ⬜ UI: Login, Play-Button, Versions-/Mod-Auswahl, Einstellungen.

**Definition of Done (Phase 2):** „Login → Play → 26.1 startet mit
DolphinClient", Launcher aktualisiert sich selbst.

---

## 4. Komponente: `backend/`

**Tech:** Node + TypeScript + Fastify, PostgreSQL, Objekt-Storage (S3/R2).

**Aufgaben Phase 3:**
1. ✅ Fastify-Server + Routen-Gerüst.
2. ⬜ `GET /v1/profile/:uuid` — Profil + aktive Cosmetics.
3. ⬜ `GET /v1/cosmetics/:uuid` — Capes/Items, die der Spieler trägt
   (vom Client beim Start abgefragt).
4. ⬜ `GET /v1/updates/:channel` — signiertes Update-Manifest für den Launcher.
5. ⬜ DB-Schema (Profile, Cosmetics-Besitz, Aktiv-Status).
6. ⬜ Identitätsprüfung über Minecraft-Services-Token (keine Passwörter).
7. ⬜ Moderation für nutzergenerierte Capes (Phase 5).

**Definition of Done (Phase 3):** Client lädt beim Start die Cosmetics eines
Spielers; andere DolphinClient-Nutzer sehen dessen Cape.

---

## 5. Komponente: `website/`

**Tech:** Next.js (App Router).

**Aufgaben Phase 4:**
1. ✅ Landingpage (Hero, Features, ehrliche Performance-Aussage).
2. ✅ `/download` — Download-Links pro OS.
3. ✅ `/account` — Platzhalter für Login/Dashboard.
4. ⬜ Account-Dashboard (Cosmetics verwalten) gegen Backend-API.
5. ⬜ Cosmetic-Store + Zahlungen (Stripe) — Phase 5.

**Definition of Done (Phase 4):** Öffentliche Seite mit Download + Account-Login.

---

## 6. Meilensteine (Reihenfolge der Umsetzung)

| M | Inhalt | Ergebnis |
|---|---|---|
| **M0** | Monorepo + alle 4 Skelette + Doku *(diese Session)* | Fundament steht, alles strukturiert |
| **M1** | Client: Module + FPS-HUD + Config lauffähig | Schneller als Vanilla, anpassbar |
| **M2** | Client: Performance-Mods (nach Lizenzprüfung) + GUI | „Lunar-Gefühl" beim Spielen |
| **M3** | Launcher: MS-Login + Spielstart | Ein Klick → 26.1 startet |
| **M4** | Launcher: Auto-Update | Selbst-aktualisierend |
| **M5** | Backend + Cosmetics + Cape-Rendering | Sichtbare Capes |
| **M6** | Website: Download + Account | Öffentlicher Release-Kandidat |
| **M7** | Store, eigene Mods, weitere Versionen (1.8.9) | Ausbau |

---

## 7. Querschnitt: Recht, Sicherheit, Kosten

Gilt durchgehend (Details in [`ROADMAP.md`](ROADMAP.md) §7–9, §13):

- **Niemals** Minecraft-Code/Assets mitliefern; Launcher lädt Original von Mojang.
- **Nur** Microsoft-Login; keine Cracked-Accounts.
- Jede gebündelte Mod-Lizenz **einzeln** prüfen (LGPL ok mit Quellhinweis; ARR nicht).
- Tokens nur in OS-Keychain; Auto-Updates signieren; Launcher code-signen.
- Fremd-Mods = Malware-Risiko → Warnungen / kuratierter Katalog.
- Laufende Kosten: Code-Signing, Server, CDN, Domain.

---

## 8. Status dieser Session (M0)

Wird in dieser Session erledigt:

- [x] Monorepo-Struktur + Root-Tooling (`package.json`, `.gitignore`)
- [x] `client/` Fabric-Mod-Skelett für 26.1 (Module, FPS-HUD, Config)
- [x] `launcher/` Electron-Skelett (Auth-/Launch-/Updater-Stubs + UI)
- [x] `backend/` Fastify-Skelett (Profil-/Cosmetics-/Update-Routen)
- [x] `website/` Next.js-Skelett (Landing, Download, Account)

Danach: M1 (Client-Module real ausbauen) als nächster Schritt.
