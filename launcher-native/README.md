# DolphinClient — Nativer Launcher (`launcher-native/`)

Der **neue Launcher** von DolphinClient: eine **echte native Anwendung in Rust**
(GUI mit [`egui`](https://github.com/emilk/egui)/`eframe`). Kein Electron, kein
Chromium, kein Node — ein einziges, kleines, blitzschnelles Programm.

Er übernimmt Microsoft- und Offline-Profile, private Cosmetics, den Download
und Start des nativen DolphinClient sowie SHA-256-geprüfte automatische Updates.

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
src/accounts.rs  Microsoft-/Import-/Offline-Profile und Vanilla-Offline-UUIDs
src/cosmetics.rs Skin-/Cape-Import, Validierung, Normalisierung und Vorschau
src/client.rs    Nativen Client + Mojang-Assets laden und als Kindprozess starten
src/game.rs      Mojang-Versionsmanifest, Client-JAR und Asset-Index laden
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

Daneben gibt es lokale Offline-Profile mit Vanillas deterministischer UUID.
Sie funktionieren nur auf bewusst entsprechend konfigurierten Offline-Mode-
Servern und können die Besitz-/Sessionprüfung eines Online-Mode-Servers nicht
umgehen. Spieldateien kommen ausschließlich von Mojang.

## Spielstart

Der Launcher lädt das Vanilla-Client-JAR als Quelle der Originalmodelle und
-texturen, den Asset-Index für Sounds und das zum exakten OS/CPU-Ziel passende
native DolphinClient-Binary. Tokens werden ausschließlich über die Umgebung an
den Kindprozess gereicht und erscheinen nicht in der Prozessliste. Lokale Skin-
und Cape-Pfade werden getrennt übergeben und bleiben auf diesem Rechner.

## Umgebungsvariablen

| Variable | Zweck |
|---|---|
| `DOLPHIN_MS_CLIENT_ID` | Eigene Azure-App-Client-ID → schaltet auf das Azure/AAD-Backend + Browser-Login um (Default: Live-Backend, offizielle Launcher-ID) |
| `DOLPHIN_MS_MODE` | Backend erzwingen: `live` (Standard) oder `azure` |
| `DOLPHIN_MS_TENANT` | AAD-Tenant für das Azure-Backend (Default `consumers`; z. B. `common`) |
| `DOLPHIN_UPDATE_MANIFEST` | Update-Manifest-URL überschreiben |
| `DOLPHIN_CLIENT_URL` | Basis-URL für native Client-Artefakte überschreiben |
| `DOLPHIN_CLIENT_BIN` | Lokales Client-Binary beim Entwickeln direkt verwenden |

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

`../release.sh` baut auf dem Linux-Release-Host Windows und Linux; auf einem Mac
baut es macOS. `publish-local.sh` erkennt zusätzlich x64/ARM64 und veröffentlicht
Launcher und Client unter getrennten Namen. Windows erhält einen NSIS-Installer;
Linux und macOS aktualisieren ihr Binary atomar. Alle Artefakte stehen mit
SHA-256 im Manifest. Für eine öffentliche Auslieferung bleibt Code-Signing
empfohlen (sonst SmartScreen/Gatekeeper).
