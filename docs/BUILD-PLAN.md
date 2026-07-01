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
| Mod-Loader | Fabric Loader **0.19.3** |
| Build-Tool | Fabric Loom **1.17-SNAPSHOT**, Gradle **9.5.1** (verifiziert) |
| Fabric API | **0.153.0+26.1.2** (Modrinth-verifiziert) |
| Mappings | Mojang Official (unobfuskiert, keine `mappings`-Zeile) |
| Loom-Workflow | Plugin `net.fabricmc.fabric-loom`, `implementation` statt `modImplementation` |
| Build-Status | **Client kompiliert gegen echtes 26.1 → `dolphinclient-0.1.0.jar`** |

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
4. ✅ **HUD-Module** (gegen echtes 26.1 kompiliert): FPS, Koordinaten+Blick­richtung,
   Uhrzeit, Sitzungszeit, Geschwindigkeit, Keystrokes. HUD über
   `HudElementRegistry` + `GuiGraphicsExtractor` (26.1-Modell), Auto-Layout.
5. ✅ **Config**: JSON in `.minecraft/config/dolphinclient.json` (Module an/aus).
6. ✅ **In-Game-Menü** (Rechte Umschalt) + **Zoom** (C), gegen die neuen
   26.1-APIs kompiliert (`KeyMappingHelper.registerKeyMapping`,
   `KeyMapping.Category.register`, `Screen`/`Button`, `Options.fov()`).
   **CPS** bleibt zurückgestellt (26.1 `MouseHandler` ohne sauberen Klick-Hook).
7. 🟡 Performance-Mods: **Lizenzprüfung erledigt** (siehe
   [`MOD-LICENSES.md`](MOD-LICENSES.md)). Empfehlung: **per Launcher von Modrinth
   nachladen** statt bündeln. **EntityCulling NICHT bündeln** (Lizenz verbietet
   Redistribution). Sodium/Lithium/Iris/ImmediatelyFast (LGPL) + FerriteCore
   (MIT) bündelbar mit Auflagen.
8. ⬜ Komfortablere Konfig-GUI + Modulpositionen verschiebbar.
9. 🟡 Cosmetics-Datenschicht (Abruf + Cache) da; Cape-**Rendering** (Textur
    laden + 26.1-Render-Layer) noch offen.

**Definition of Done (Phase 1):** Mod kompiliert mit Java 25, lädt in 26.1,
zeigt FPS-HUD, Module per Konfig an/abschaltbar.

> ⚠️ **Hinweis zum Container:** Hier läuft Java 21; 26.1 braucht Java 25. Das
> Skelett ist korrekt für 26.1 eingerichtet, der finale Build erfolgt lokal
> mit JDK 25.

---

## 3. Komponente: `launcher/`

**Tech:** Electron + TypeScript. Auto-Update via `electron-updater`.

**Aufgaben Phase 2:**
1. ✅ Electron-Grundgerüst (Haupt-/Renderer-Prozess, Build-Skripte, Renderer-Copy).
2. ✅ **Microsoft-OAuth** (Device-Code-/Link-Code-Flow) → Xbox Live → XSTS →
   Minecraft-Services → Profil. Refresh-Token in OS-Keychain (safeStorage).
   **Nur legitimer Login, keine Cracked-Accounts.**
3. ✅ **Spielstart (code-vollständig)**: Manifest + 26.1-Versions-JSON, Client-JAR,
   Libraries (OS-Regeln), Assets (Index+Objekte), Natives-Extraktion,
   **Fabric-Profil-Merge**, Classpath + JVM-/Game-Args, JDK-25-Start.
   Runtime nicht getestet (kein 26.1+Grafik hier); offen: Mod-Jar in `mods/`.
4. ✅ **Auto-Update** (electron-updater, generischer Feed vom Backend) +
   electron-builder-Paketierung. Vor Release: Code-Signing + echte Signaturen.
5. ✅ UI: Login, Play-Button, Anzeige des Link-Codes. (Versions-/Mod-Auswahl folgt.)

**Definition of Done (Phase 2):** „Login → Play → 26.1 startet mit
DolphinClient", Launcher aktualisiert sich selbst.

---

## 4. Komponente: `backend/`

**Tech:** Node + TypeScript + Fastify, PostgreSQL, Objekt-Storage (S3/R2).

**Aufgaben Phase 3:**
1. ✅ Fastify-Server + Routen-Gerüst.
2. ✅ `GET /v1/profile/:uuid` — Profil + aktive Cosmetics (In-Memory-Store).
3. ✅ `GET /v1/cosmetics/:uuid` + `GET /` (Liste) + `POST /:uuid/active` —
   Cape setzen/abrufen (In-Memory; DB/CDN folgt).
4. 🟡 `GET /v1/updates/:channel` (+ `/latest.yml` für electron-updater) —
   wohlgeformter Feed; echte Signaturen/Hashes kommen aus dem Release-Build.
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
4. ✅ Account-Dashboard (Cosmetics verwalten) gegen Backend-API (Dev: UUID-Feld;
   Web-Login über Microsoft folgt). Backend mit CORS.
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
