# DolphinClient — Konzept, Architektur & Roadmap

Dieses Dokument ist die technische und strategische Grundlage für
DolphinClient: ein performance-orientierter Minecraft-Client (Java Edition)
mit eigenem Launcher, Auto-Updates, Cosmetics, Website und erweiterbarem
Mod-System.

Es ist bewusst ehrlich geschrieben — inklusive der Stellen, an denen das
Vorhaben schwierig, teuer oder rechtlich heikel ist.

---

## 1. Vision & Ziele

**Vision:** Ein Client, der sich anfühlt wie „ein Klick und Minecraft läuft
schneller und schöner als vorher" — mit Cosmetics und Community wie bei den
großen Anbietern.

**Konkrete Ziele:**

1. Spürbar mehr FPS als Vanilla, out-of-the-box vorkonfiguriert.
2. Eigener Launcher mit Microsoft-Login, Spielstart und Auto-Update.
3. Cosmetics (Capes, später mehr) inkl. Backend und Account-System.
4. Website mit Download und Account-Bereich.
5. Erweiterbares Mod-System: viele optionale Mods, die **nur Leistung kosten,
   wenn sie aktiviert sind**; User sollen eigene Mods hinzufügen können.

**Nicht-Ziel (wichtig):** „Mehr FPS als jeder andere Client." Siehe Abschnitt 3
— das ist als messbares Versprechen praktisch nicht haltbar. Wir verkaufen
**Komfort + Cosmetics + Community**, nicht eine FPS-Zahl, die die Konkurrenz
schlägt.

---

## 2. Versions-Strategie

### Zielversion: 26.1 (bestätigt)

Minecraft Java **26.1 „Tiny Takeover"** wurde am **24. März 2026**
veröffentlicht. Mojang ist auf das Schema **`Jahr.Drop.Hotfix`** umgestiegen
(angekündigt Dez. 2025) — daher „26.1" statt „1.x.y".

Für DolphinClient besonders relevant:

- **26.1 ist die erste vollständig unobfuskierte Version.** Es gibt **keine
  Obfuskations-Mappings mehr** → der Mod-Workflow wird deutlich einfacher
  (Mojang Official Names direkt, kein Remapping, `implementation` statt
  `modImplementation`).
- Benötigt **Java 25** (vorher 17/21).
- RAM-Default jetzt **4 GB** (vorher 2 GB).
- **Fabric unterstützt 26.1** (Loader 0.18.4, Loom 1.15, Gradle 9.4).

Die exakte Versionsnummer bleibt ein Build-Parameter (`gradle.properties`),
damit ein Sprung auf 26.2+ ein Einzeiler ist.

### Empfehlung: mit EINER Version starten

Modern (Fabric) und 1.8.9 sind technisch fast zwei getrennte Projekte. Alles —
Cosmetics, UI, Mod-System, Auto-Update — muss **pro Version doppelt gebaut und
gepflegt** werden. Das ist der größte versteckte Kostenfaktor.

| | 26.1 (Zielversion) | 1.8.9 |
|---|---|---|
| Mod-Loader | **Fabric** (leichtgewichtig, top gepflegt) | Legacy Fabric / Ornithe / Forge (fummelig) |
| Mappings | **keine** (unobfuskiert) | Obfuskiert, MCP/Yarn nötig |
| Performance-Mods | Sodium, Lithium, Iris, FerriteCore — alle aktuell | nur Backports, lückenhaft |
| Zielgruppe | normale Spieler, neue Features | **PvP (Bedwars, Hypixel)** |
| Wartungsaufwand | normal | hoch (alte Codebasis, wenig Tooling) |

**Faustregel:** Ist die Zielgruppe PvP, ist 1.8.9 sogar wichtiger als die
neueste Version (deshalb sind Lunar/Badlion dort stark). Trotzdem: **eine**
Version auf Top-Niveau bringen, dann die zweite ergänzen — nicht beide
gleichzeitig.

---

## 3. Der FPS-Realitäts-Check

Das ist der wichtigste Abschnitt, weil das ganze Projekt sonst auf einem
falschen Versprechen steht.

Die großen FPS-Gewinne in modernem Minecraft kommen aus **quelloffenen Mods**,
die jeder frei nutzen kann:

- **Sodium** — neue Rendering-Engine, der mit Abstand größte Hebel (oft 2–5×).
- **Lithium** — Optimierung der Spiel-/Tick-Logik.
- **FerriteCore** — deutlich weniger RAM-Verbrauch.
- **EntityCulling**, **ImmediatelyFast**, **ModernFix** — weitere Gewinne.
- **Iris** — Shader-Support (auf Sodium aufbauend).

Konsequenz: Ein Client wie Lunar **bündelt** im Kern genau solche
Optimierungen. **Sodium nennenswert zu schlagen ist unrealistisch** — es ist
bereits nahe am technischen Optimum. Dein Client wird ungefähr so schnell sein
wie „Fabric + ein gut konfiguriertes Sodium-Paket".

**Daraus folgt das echte Verkaufsargument:**

1. **Bequemlichkeit** — alles vorkonfiguriert, ein Klick, läuft.
2. **Cosmetics & Community** — das emotionale Bindeglied (und die Monetarisierung).
3. **PvP-/QoL-Features** — FPS-Overlay, Keystrokes, Zoom, CPS, etc.
4. **Verlässliche Updates & Support.**

> **Zum Wunsch „Mods kosten nur Leistung, wenn sie an sind":** technisch
> umsetzbar, aber mit einer Einschränkung. Fabric lädt Mods beim Start.
> „Aus" heißt in der Praxis: Feature/Mixin per Konfiguration deaktiviert,
> sodass kein Code-Pfad mehr läuft — manche Mods brauchen dafür einen
> Neustart. **Echtes Hot-Toggle ohne Neustart geht nur begrenzt.** Siehe
> Abschnitt 5.

---

## 4. Technische Architektur

Vier Komponenten, klar getrennt:

```
┌──────────────────────────────────────────────────────────────┐
│  website/   Next.js — Marketing, Download, Account, Store      │
└───────────────┬──────────────────────────────────────────────┘
                │  HTTPS / REST
┌───────────────▼──────────────────────────────────────────────┐
│  backend/   Accounts · Cosmetics-Besitz · Update-Feed · Store  │
│             PostgreSQL · Objekt-Storage (Capes) · CDN          │
└───────────────▲───────────────────────────▲──────────────────┘
                │  Update-Manifest           │  Cosmetics-Daten
┌───────────────┴────────┐      ┌────────────┴──────────────────┐
│  launcher/             │      │  client/  (der Mod im Spiel)   │
│  MS-Login · Spielstart │ ───► │  Fabric · Sodium-Paket · Mods  │
│  Auto-Update           │ lädt │  Cosmetics-Renderer · Config   │
└────────────────────────┘      └───────────────────────────────┘
```

### 4.1 Client (der eigentliche Mod)

- **Sprache/Loader:** Java + **Fabric Loader** + **Fabric API** + **Mixin**.
- **Build:** Gradle mit **Fabric Loom 1.15** (unobfuskierter Workflow).
  Mappings: **Mojang Official** (26.1 ist unobfuskiert; Yarn ist abgekündigt).
- **Gebündelte Performance-Mods:** Sodium, Lithium, FerriteCore, EntityCulling,
  ImmediatelyFast, Iris (optional). ⚠️ **Lizenzen pro Mod prüfen** — die
  meisten sind LGPL/MIT, aber das muss vor dem Bündeln einzeln verifiziert
  werden (siehe Abschnitt 8).
- **Eigener Code:** Cosmetics-Renderer, Mod-Verwaltung/Toggle-System,
  In-Game-Konfig-GUI (z. B. via Cloth Config), HUD-Module (FPS, Koordinaten,
  Keystrokes, CPS).
- **Auth-Daten:** Der Client bekommt die Session-Tokens **vom Launcher** —
  er macht keinen eigenen Login.

### 4.2 Launcher

Aufgaben: Microsoft-Login, Verwaltung lokaler Versionen, Download der
Original-Minecraft-Dateien **von Mojang** (niemals selbst gehostet!),
Zusammenbau des Classpath, JVM-Start, Auto-Update des Clients.

- **Option A — Tauri (Rust + Web-UI):** klein, schnell, sicher. Etwas mehr
  Rust-Aufwand.
- **Option B — Electron (TypeScript):** schnellster Einstieg für Web-Devs,
  ausgereifte Auto-Update-Werkzeuge (`electron-updater`), aber größere Binärdatei.

**Empfehlung:** Electron für den MVP (schnellstes Ergebnis), Tauri als
mögliche spätere Optimierung.

- **Microsoft-Auth:** OAuth 2.0 Device-Code- oder Auth-Code-Flow gegen
  Microsoft/Xbox Live → Minecraft-Services. **Nur legitimer MS-Login, keine
  Cracked-Accounts.** Tokens sicher speichern (OS-Keychain), Refresh sauber
  behandeln.
- **Spielstart:** Version-Manifest, Library-/Asset-Downloads und JVM-Argumente
  sind gut dokumentiert, aber nicht trivial. Bestehende Open-Source-Logik als
  Vorbild nehmen, statt blind neu zu erfinden.

### 4.3 Backend

- **Eine API** (Node/TypeScript **oder** Go). PostgreSQL als Datenbank.
- **Dienste:**
  - *Account/Profil:* verknüpft die Minecraft-UUID mit Cosmetics-Besitz und
    Einstellungen. (Der Login selbst läuft über Microsoft — wir prüfen die
    Identität, speichern keine Passwörter.)
  - *Cosmetics:* welche Capes/Items ein Nutzer besitzt + aktiv hat; Texturen
    in Objekt-Storage (S3/Cloudflare R2) hinter einem CDN.
  - *Update-Feed:* signiertes Versions-Manifest, das der Launcher abfragt.
  - *Store* (später): Käufe, via Zahlungsanbieter (z. B. Stripe).
- **Moderation:** für nutzergenerierte Inhalte (eigene Capes) nötig.

### 4.4 Website

- **Next.js:** Landingpage, Download, Account-Dashboard, später Cosmetic-Store.
- Teilt sich die API mit dem Backend.

---

## 5. Mod-System — „nur aktiv = nur dann Leistung"

Ziel: viele optionale Mods; deaktivierte Mods sollen **keine** Leistung kosten;
User sollen eigene Mods hinzufügen können.

**Design:**

1. **Eigene Module** (HUD, QoL, PvP-Features) werden über eine zentrale
   Registry verwaltet und sind per Konfig **einzeln an/aus**. Sind sie aus,
   läuft ihr Code-Pfad nicht (Mixin-Injection per `@Inject` mit Guard bzw.
   bedingte Aktivierung), also **kein Leistungskosten** im Aus-Zustand.
   Manche Mixins benötigen für echtes „Aus" einen Neustart.
2. **Fremd-/Drittanbieter-Mods (.jar):** ein verwalteter Ordner; der User
   legt Fabric-Mods hinein. Diese werden vom Fabric Loader beim Start geladen.
   - ⚠️ **Echtes Hot-Toggle ohne Neustart ist hier nicht zuverlässig möglich** —
     das Aktivieren/Deaktivieren erfordert in der Regel einen Neustart.
   - ⚠️ **Sicherheit:** Fremd-Mods sind beliebiger Java-Code mit vollen
     Rechten. Das ist ein Malware-Einfallstor. Maßnahmen: deutliche Warnungen,
     optional kuratierter/geprüfter Mod-Katalog, Signatur-Prüfung für
     „verifizierte" Mods. (JVM-Sandboxing ist nur eingeschränkt möglich.)

**Realistische Formulierung fürs Marketing:** „Deaktivierte Module verbrauchen
keine Ressourcen" — ja. „Beliebige Fremd-Mods per Schalter ohne Neustart
an/aus" — nur eingeschränkt; ehrlich kommunizieren.

---

## 6. Cosmetics-System

- **Capes zuerst** (einfachster, gefragtester Cosmetic-Typ).
- Client rendert die Cosmetics anhand der Daten, die er beim Start vom Backend
  zur Spieler-UUID lädt; sichtbar für andere DolphinClient-Nutzer.
- Texturen im Objekt-Storage + CDN; Besitz/Aktiv-Status in der DB.
- Später: Wings, Bandanas, Hats, animierte Capes, eigene hochgeladene Capes
  (mit **Moderation**).
- ⚠️ Cosmetics dürfen serverseitig **nicht wie Cheats wirken** (siehe
  Abschnitt 7).

---

## 7. Anti-Cheat, Server-Allowlists, Bans

Server wie Hypixel führen Allowlists erlaubter Clients/Mods. Risiken:

- Cosmetics oder Module, die wie ein unfairer Vorteil aussehen, können zu
  **Bans der Nutzer** führen.
- Manche Server erkennen Clients und verbieten unbekannte.

**Maßnahmen:** keine verbotenen Funktionen (kein Reach, kein Auto-Clicker als
Default, etc.), saubere Modul-Auswahl, ggf. **frühzeitig Kontakt zu großen
Servern** suchen, um auf die Allowlist zu kommen.

---

## 8. Rechtliche Checkliste (der härteste Gate)

- **Mojang-EULA / Brand Guidelines:**
  - **Minecraft-Code und -Assets niemals mitliefern.** Der Launcher lädt die
    Original-Dateien von Mojang; der User muss das Spiel besitzen.
  - **Nur Microsoft-Login, keine Cracked-Accounts.** Andernfalls rechtlich und
    community-mäßig sofort erledigt.
  - Marken-/Namensregeln einhalten (Logo, „Minecraft"-Nutzung, Disclaimer
    „nicht von Mojang/Microsoft").
- **Mod-Lizenzen:** Jede gebündelte Mod **einzeln** prüfen. LGPL ist meist
  okay, wenn man die Mod als separates, unverändertes Modul mitliefert,
  Quelle/Lizenz beilegt und Änderungen offenlegt. **„All Rights Reserved"- und
  restriktiv lizenzierte Mods dürfen ohne Erlaubnis nicht gebündelt werden.**
  Konkrete Prüfung der geplanten Mods: [`MOD-LICENSES.md`](MOD-LICENSES.md)
  (u. a. **EntityCulling darf nicht gebündelt werden**). Empfehlung: Mods per
  Launcher von Modrinth nachladen statt mitliefern.
- **Account-/Datenschutz:** Tokens sicher speichern; DSGVO beachten
  (Account-Daten, Käufe).
- **Zahlungen/Store:** Steuern, Widerruf/Rückerstattung, Jugendschutz.
- Empfehlung: vor Release **rechtliche Beratung** zu EULA, Marken und
  Mod-Lizenzen einholen.

---

## 9. Sicherheit

- **Account-Schutz:** OAuth-Tokens nur in der OS-Keychain, niemals im Klartext;
  Refresh-Tokens sauber rotieren.
- **Fremd-Mods:** beliebiger Code → Warnhinweise, optional kuratierter Katalog,
  Signaturprüfung verifizierter Mods.
- **Auto-Update:** Artefakte **signieren** und im Launcher die Signatur prüfen,
  damit niemand bösartige Updates unterschieben kann.
- **Code-Signing:** Launcher-Binärdateien signieren (sonst SmartScreen-/
  Gatekeeper-Warnungen). Kostet jährlich Geld, ist aber faktisch Pflicht.

---

## 10. Tech-Stack — Zusammenfassung der Empfehlungen

| Bereich | Empfehlung | Begründung |
|---|---|---|
| Client-Loader | Fabric + Mixin | Leichtgewichtig, beste Performance-Mods |
| Client-Build | Gradle + Fabric Loom | Standard im Fabric-Ökosystem |
| Mappings | Yarn oder Mojang Official | Gut dokumentiert |
| Launcher | Electron (MVP) → ggf. Tauri | Schnellster Start; Auto-Update ausgereift |
| Auth | Microsoft OAuth 2.0 | Einzig zulässiger, sicherer Weg |
| Backend | Node/TS oder Go + PostgreSQL | Schnelle Entwicklung, robust |
| Storage/CDN | S3/Cloudflare R2 + CDN | Günstig, skaliert |
| Website | Next.js | Marketing + App in einem |
| Zahlungen | Stripe (später) | Standard, gut dokumentiert |

---

## 11. Roadmap (Phasen)

> Prinzip: in jeder Phase etwas **Lauffähiges**, statt monatelang an einem
> großen System ohne Ergebnis zu bauen.

**Phase 0 — Fundament & Recht** *(bevor Code entsteht)*
- Zielversion endgültig festlegen (Klärung „26.1").
- Projektname/Marke prüfen, Domain, Disclaimer.
- Mod-Lizenzen der geplanten Bundles prüfen.

**Phase 1 — Client-MVP** *(das Herzstück zuerst)*
- Fabric-Mod-Grundgerüst + Build.
- Kuratiertes Sodium/Lithium/FerriteCore/EntityCulling-Paket einbinden.
- Erste eigene Module: FPS-Overlay, Koordinaten, Zoom, Keystrokes.
- In-Game-Konfig-GUI + an/aus-Modulsystem.
- **Ergebnis:** läuft, ist deutlich schneller als Vanilla, manuell startbar.

**Phase 2 — Launcher**
- Microsoft-Login, Download der Mojang-Dateien, Spielstart.
- Auto-Update für den Client (signiert).
- **Ergebnis:** „ein Klick → Minecraft startet mit DolphinClient".

**Phase 3 — Backend & Cosmetics**
- Account/Profil-Service, Cosmetics-Besitz, Cape-Rendering im Client.
- Storage + CDN, einfache Moderation.
- **Ergebnis:** sichtbare Capes für DolphinClient-Nutzer.

**Phase 4 — Website**
- Landingpage, Download, Account-Dashboard.
- **Ergebnis:** öffentlicher Download + Selbstverwaltung.

**Phase 5 — Ausbau**
- Cosmetic-Store + Zahlungen.
- Eigene Mods hinzufügen (kuratierter Katalog + lokaler Ordner).
- Weitere Cosmetics-Typen.
- **Zweite Minecraft-Version** (z. B. 1.8.9), wenn Nachfrage da ist.

---

## 12. Risiken & offene Fragen

| Thema | Risiko / Frage |
|---|---|
| Version 26.1 | Bestätigt. Braucht Java 25; 26.2 ist bereits raus (Bump einplanen). |
| FPS-Versprechen | „Mehr als alle" nicht haltbar; Positionierung anpassen. |
| Recht | EULA, Marken, Mod-Lizenzen, Cracked-Verbot — vor Release klären. |
| Team & Zeit | Lunar-Niveau = Mann-Jahre; solo sehr schwer. Mitstreiter? |
| Laufende Kosten | Server, CDN, Code-Signing, Domain — Budget? |
| Server-Bans | Allowlist-Koordination mit großen Servern nötig. |
| Fremd-Mods | Sicherheits-/Malware-Risiko; Toggle ohne Neustart begrenzt. |

---

## 13. Laufende Kostenfaktoren (grobe Orientierung)

- **Code-Signing-Zertifikate** (Windows/macOS): jährlich dreistellig.
- **Server/Backend + Datenbank:** monatlich, skaliert mit Nutzerzahl.
- **Objekt-Storage + CDN:** günstig am Anfang, wächst mit Downloads.
- **Domain + E-Mail.**
- **Optional:** Apple Developer Program (für macOS-Notarisierung).

---

## 14. Nächster konkreter Schritt

Zielversion **26.1 ist bestätigt**, **Phase 1 läuft**: das Fabric-Mod-Grundgerüst
mit Build-Setup und erstem HUD-Modul wird angelegt. Der konkrete, ausführliche
Umsetzungsplan steht in **[`BUILD-PLAN.md`](BUILD-PLAN.md)**.
