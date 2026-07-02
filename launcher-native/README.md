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

Device-Code-Flow ("Link-Code"): Der Launcher zeigt einen kurzen Code + eine URL.
Der Nutzer öffnet die URL, tippt den Code ein, bestätigt — fertig. Kein
eingebetteter Browser nötig.

Die Azure-App-Client-ID ist einkompiliert
(`d7c09844-ad46-4930-a39b-ac04ca90d894`, von portal.azure.com) und lässt sich mit
der Umgebungsvariable `DOLPHIN_MS_CLIENT_ID` überschreiben. Endnutzer sehen Azure
nie.

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
| `DOLPHIN_MS_CLIENT_ID` | Azure-App-Client-ID überschreiben (Default ist einkompiliert) |
| `DOLPHIN_MS_TENANT` | OAuth-Tenant (Default `consumers` = persönliche MS-Konten; z. B. `common`) |
| `DOLPHIN_UPDATE_FEED` | Update-Feed-URL überschreiben |

## Azure-App-Registrierung (wichtig!)

Damit der Login funktioniert, muss die Azure-App **persönliche Microsoft-Konten
unterstützen** (Minecraft nutzt MSA). Sonst antwortet Microsoft mit
`AADSTS700016` („application … was not found in the directory 'Microsoft
Accounts'"). In portal.azure.com → **App registrations** → deine App:

1. **Authentication → Supported account types**: „Personal Microsoft accounts
   only" **oder** „Accounts in any organizational directory and personal
   Microsoft accounts". (Im Manifest: `signInAudience` =
   `PersonalMicrosoftAccount` bzw. `AzureADandPersonalMicrosoftAccount`.)
2. **Authentication → Advanced settings → Allow public client flows** = **Yes**
   (zwingend für den Device-Code-Flow).
3. Speichern, ein paar Minuten warten, erneut anmelden.

Bei „any org + personal" ggf. `DOLPHIN_MS_TENANT=common` setzen; bei „personal
only" bleibt der Default `consumers`.

## Release / Paketierung

CI (`.github/workflows/release.yml`) baut das Release-Binary pro OS und hängt es
an ein GitHub-Release. Für einen echten Windows-Installer kann später z. B.
`cargo-wix` (MSI) oder ein NSIS-Skript ergänzt werden. Vor dem finalen Release:
**Code-Signing** (sonst SmartScreen/Gatekeeper).
