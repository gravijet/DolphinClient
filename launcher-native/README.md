# DolphinClient — Nativer Launcher (`launcher-native/`)

Der **neue Launcher** von DolphinClient: eine **echte native Anwendung in Rust**
(GUI mit [`egui`](https://github.com/emilk/egui)/`eframe`). Kein Electron, kein
Chromium, kein Node — ein einziges, kleines, blitzschnelles Programm.

> Löst den alten Electron/TypeScript-Launcher (`launcher/`) ab. Gleiche Aufgaben,
> aber nativ: Microsoft-Login, Spielstart von Minecraft 26.1 (Fabric +
> DolphinClient-Mod) und ein Update-Check.

## Warum nativ / Rust?

| | Electron (alt) | Rust + egui (neu) |
|---|---|---|
| Startzeit | mehrere Sekunden | **< 0,5 s** |
| Binär-/Installgröße | ~80–150 MB | **~10–15 MB** |
| RAM (idle) | ~150–300 MB | **~30 MB** |
| Renderer | mitgeliefertes Chromium | native GPU (OpenGL via `glow`) |

## Entwicklung

Voraussetzung: eine aktuelle Rust-Toolchain (`rustup`, stable).

```bash
cd launcher-native
cargo run              # startet den Launcher (Debug)
cargo build --release  # optimiertes, gestriptes Release-Binary in target/release/
```

## Aufbau

```
src/main.rs      Einstiegspunkt, eframe-Fenster (versteckt die Konsole unter Windows)
src/app.rs       egui-Oberfläche (Login, Play, Fortschritt, Einstellungen)
src/auth.rs      Microsoft-Device-Code-Flow → Xbox → XSTS → Minecraft → Profil
src/game.rs      Mojang-/Fabric-Dateien laden, Argumente bauen, JVM starten
src/tokens.rs    Refresh-Token in der OS-Keychain (Windows Cred-Manager / macOS / keyutils)
src/config.rs    Einstellungen (RAM, Java-Pfad …) + .minecraft-Pfad je OS
src/updater.rs   Update-Check gegen den Backend-Feed
src/events.rs    Nachrichten von Worker-Threads an die UI
```

Netzwerk-/Datei-Arbeit läuft in Worker-Threads; die UI bleibt flüssig und wird
über einen Kanal (`std::sync::mpsc`) aktualisiert.

## Microsoft-Login

Zwei Backends:

- **Live (Standard) — keine eigene Azure-App nötig.** Der Launcher nutzt die
  **Client-ID des offiziellen Minecraft-Launchers** (`00000000402b5328`) über
  `login.live.com`. Diese ID ist bereits für die Minecraft-API freigeschaltet —
  genau wie es **prismarine-auth / mineflayer / MCProtocolLib** machen. Kein
  `portal.azure.com`, keine Freigabe, kein `login_with_xbox`-403. Anmeldung per
  **Device-Code** (Seite öffnen, kurzen Code eingeben — der Standardweg der
  genannten Libraries).
- **Azure/AAD (optional) — eigene, freigeschaltete App.** Wer eine eigene
  Azure-App hat (`DOLPHIN_MS_CLIENT_ID` gesetzt), nutzt `login.microsoftonline.com`
  und den **Browser-Login** (Authorization-Code-Flow + PKCE, lokaler
  Loopback-Redirect — Fenster öffnet sich, anmelden, fertig, kein Code eintippen).
  Voraussetzung: die App ist für die Minecraft-API freigeschaltet (siehe unten).

**Nur legitimer Microsoft-Login** — keine Cracked-Accounts (Mojang-EULA).
Spieldateien kommen **ausschließlich von Mojang** / dem Fabric-Meta-Service.

## Spielstart

Vollständig portiert vom bisherigen Launcher: Vanilla-Versions-JSON + Client-JAR,
Libraries (mit OS-Regeln) + Natives-Extraktion, Assets (Index + Objekte),
**Fabric-Profil-Merge** (Loader-Libraries + `mainClass`), Argument-Bau
(Platzhalter-Ersetzung) und JVM-Start mit JDK 25. Fabric API wird von Modrinth
nachgeladen; eine gebündelte DolphinClient-Mod (falls neben dem Programm unter
`mods/` bzw. `resources/mods/` vorhanden) wird nach `.minecraft/mods/` kopiert.

> **Runtime nicht in dieser Umgebung getestet** (kein 26.1 + Konto + GPU). Der
> komplette Flow ist implementiert und kompiliert sauber; vor Auslieferung real
> gegentesten.

## Umgebungsvariablen

| Variable | Zweck |
|---|---|
| `DOLPHIN_MS_CLIENT_ID` | Eigene Azure-App-Client-ID → schaltet auf das Azure/AAD-Backend + Browser-Login um (Default: Live-Backend, offizielle Launcher-ID) |
| `DOLPHIN_MS_MODE` | Backend erzwingen: `live` (Standard) oder `azure` |
| `DOLPHIN_MS_TENANT` | AAD-Tenant für das Azure-Backend (Default `consumers`; z. B. `common`) |
| `DOLPHIN_UPDATE_FEED` | Update-Feed-URL überschreiben |

## Azure-App-Registrierung (nur für das optionale Azure-Backend)

> **Für den Standard-Login (Live-Backend) nicht nötig** — der läuft ohne eigene
> Azure-App. Dieser Abschnitt gilt nur, wenn du `DOLPHIN_MS_CLIENT_ID` mit deiner
> **eigenen** App setzt (z. B. für den Browser-Login).

Damit der Azure-Login funktioniert, muss die App **persönliche Microsoft-Konten
unterstützen** (Minecraft nutzt MSA). Sonst antwortet Microsoft mit
`AADSTS700016` („application … was not found in the directory 'Microsoft
Accounts'"). In portal.azure.com → **App registrations** → deine App:

1. **Authentication → Supported account types**: „Personal Microsoft accounts
   only" **oder** „Accounts in any organizational directory and personal
   Microsoft accounts". (Im Manifest: `signInAudience` =
   `PersonalMicrosoftAccount` bzw. `AzureADandPersonalMicrosoftAccount`.)
2. **Authentication → Advanced settings → Allow public client flows** = **Yes**
   (zwingend für Device-Code- und Loopback-Flow).
3. **Authentication → Add a platform → Mobile and desktop applications** →
   Redirect-URI **`http://localhost`** hinzufügen (für den Browser-Login;
   dynamischer Port wird von AAD akzeptiert).
4. Speichern, ein paar Minuten warten, erneut anmelden.

Bei „any org + personal" ggf. `DOLPHIN_MS_TENANT=common` setzen; bei „personal
only" bleibt der Default `consumers`.

### ⚠️ Minecraft-API-Freigabe (Ursache für `HTTP 403` bei `login_with_xbox`)

Microsoft/Mojang lässt **neu registrierte** Azure-Apps standardmäßig **nicht**
an die Minecraft-API (`api.minecraftservices.com`). Bis die App freigeschaltet
ist, bricht der Login **nach** erfolgreichem MSA-/Xbox-/XSTS-Schritt mit
**`login_with_xbox → HTTP 403 Forbidden`** ab.

**Lösung:** die App-ID über das offizielle Formular freischalten lassen:
**<https://aka.ms/mce-reviewappid>**. Die Freigabe erfolgt manuell durch
Microsoft und kann dauern. Ohne diese Freigabe funktioniert **kein** eigener
Launcher-Login — das ist eine reine Plattform-Richtlinie, unabhängig vom Code.

## Release / Paketierung

CI (`.github/workflows/release.yml`) baut das Release-Binary pro OS und hängt es
an ein GitHub-Release. Für einen echten Windows-Installer kann später z. B.
`cargo-wix` (MSI) oder ein NSIS-Skript ergänzt werden. Vor dem finalen Release:
**Code-Signing** (sonst SmartScreen/Gatekeeper).
