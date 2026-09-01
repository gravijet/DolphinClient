# DolphinClient — Build-Anleitung (alles lokal, ohne GitHub)

Diese Anleitung beschreibt **Schritt für Schritt**, wie du aus dem Quellcode
eine fertige, startbare **App** baust. Es wird **nichts mehr auf GitHub
gebaut** — alles passiert auf deinem Rechner.

Es gibt **zwei** Programme:

| Programm | Ordner | Das ist … | Sprache / Toolchain |
|---|---|---|---|
| **Launcher** | `launcher-native/` | **die App**, die man doppelklickt: Login, Start, Auto-Update | Rust **stable** |
| **Client** | `client-rust/` | das eigentliche Spiel (nativer Minecraft-26.1-Client) | Rust **nightly** |

Der Launcher lädt den Client normalerweise von der Website nach. Zum lokalen
Testen sagst du dem Launcher per Umgebungsvariable, dass er **deinen** lokal
gebauten Client nehmen soll (siehe [Abschnitt 5](#5-launcher-mit-lokalem-client-testen)).

> **Windows-`.exe` von Linux aus bauen:** Entgegen früherer Annahme lassen sich
> **beide** Programme (eframe-Launcher *und* wgpu/azalea-Client) sauber von Linux
> aus nach Windows cross-compilen — über das `x86_64-pc-windows-gnu`-Target mit
> mingw-w64. Das ist der Standardweg auf dem Server (dieser Rechner) und wird in
> [Abschnitt 7.1](#71-windows-exe-von-linux-aus-cross-bauen--veröffentlichen)
> beschrieben. Für macOS gilt weiterhin: auf einem Mac bauen.

---

## Der einfachste Weg: ein einziger Befehl

Wenn du einfach **alles** bauen und veröffentlichen willst — Client, Launcher,
Website, Downloads und den Push zu GitHub — brauchst du nur **ein** Skript:

```bash
./release.sh
```

Du musst dabei **nichts schreiben**: Das Skript schlägt die nächste
Versionsnummer vor (Enter genügt) und **erstellt den Changelog automatisch**
aus den geänderten Dateien (du kannst ihn mit Enter übernehmen oder auf Wunsch
selbst tippen). Danach zeigt es eine Zusammenfassung, und nach **einer**
Bestätigung erledigt es den Rest allein:

1. setzt die Dateirechte im Repo zurück (verhindert Rechte-Fehler beim Build),
2. setzt die Version überall (`package.json`, beide `Cargo.toml`, Lockfile),
3. trägt den automatischen Changelog auf der Website ein,
4. baut auf Linux **Windows + Linux**, auf einem Mac **macOS**, jeweils Launcher
   und Client, und bei Bedarf die Website,
5. veröffentlicht Downloads + Manifest und schaltet die Website live,
6. committet alles und pusht nach GitHub.

Während des Bauens siehst du **keine** Log-Flut, sondern zwei Fortschritts­balken
(aktueller Schritt + Gesamt) mit genutzter und erwarteter Dauer — die Ausgabe
kommt erst gebündelt, wenn ein Schritt fertig ist.

**Es bricht nie einfach ab.** Geht ein Schritt schief, fragt es, ob es den
Schritt **wiederholen**, **alles von vorne** machen oder (nur mit deiner
ausdrücklichen Bestätigung) **abbrechen** soll.

Nützliche Optionen: `./release.sh --linux`, `./release.sh --macos` (auf einem
Mac), `./release.sh --no-windows`,
`./release.sh --no-publish` (nur bauen/committen), `./release.sh -y` (ohne
Rückfrage). Alle Optionen: `./release.sh --help`.

> Der Rest dieser Anleitung erklärt die **einzelnen** Schritte von Hand — für
> den Fall, dass mal etwas schiefgeht oder du nur einen Teil bauen willst.

---

## 0. Überblick der Befehle (Kurzfassung)

Wenn du es eilig hast und die Voraussetzungen schon hast:

```bash
# im Repo-Wurzelverzeichnis
cd launcher-native && cargo build --release && cd ..     # -> die App
cd client-rust     && cargo build --release && cd ..     # -> der Client
```

Ergebnis:

- **App/Launcher:** `launcher-native/target/release/dolphinclient-launcher` (Windows: `dolphinclient-launcher.exe`)
- **Client:** `client-rust/target/release/dolphinclient` (Windows: `dolphinclient.exe`)

Der Rest dieses Dokuments erklärt jeden Schritt genau, inklusive Testen und
Veröffentlichen.

---

## 1. Voraussetzungen installieren

### 1.1 Rust (rustup) — für alle Betriebssysteme

Rust wird über **rustup** installiert. Die passende Nightly-Toolchain für den
Client zieht sich rustup **automatisch**, weil `client-rust/rust-toolchain.toml`
sie festlegt — du musst also nichts von Hand umschalten.

- **Windows:** [https://rustup.rs](https://rustup.rs) öffnen, `rustup-init.exe`
  herunterladen und ausführen. Zusätzlich die **Visual Studio Build Tools** mit
  „Desktopentwicklung mit C++" installieren (der MSVC-Linker wird für Rust auf
  Windows gebraucht).
- **macOS:**
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  xcode-select --install     # Compiler/Linker von Apple
  ```
- **Linux:**
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```

Danach neues Terminal öffnen (oder `source $HOME/.cargo/env`) und prüfen:

```bash
rustc --version
cargo --version
```

### 1.2 System-Build-Pakete für Linux

Nur unter Linux nötig (Windows/macOS brauchen das nicht).

**Für den Launcher** (`launcher-native`, GTK/eframe):

```bash
sudo apt-get update
sudo apt-get install -y libgtk-3-dev libxcb-render0-dev libxcb-shape0-dev \
  libxcb-xfixes0-dev libxkbcommon-dev
```

**Für den Client** (`client-rust`, winit/wgpu):

```bash
sudo apt-get install -y libx11-dev libxkbcommon-dev libwayland-dev \
  libxrandr-dev libxi-dev libxcursor-dev libgl1-mesa-dev
```

(Auf Fedora/Arch heißen die Pakete anders — sinngemäß X11-, Wayland-,
xkbcommon-, GTK3- und Mesa/GL-Entwicklungspakete.)

### 1.3 Node.js — nur fürs Veröffentlichen

Wird **nur** gebraucht, wenn du die Download-Seite aktualisierst (Schritt 7),
weil `deploy/gen-manifest.mjs` in Node läuft. Zum reinen Bauen/Testen **nicht**
nötig. Node 18+ genügt.

---

## 2. Den Launcher bauen (die App)

```bash
cd launcher-native
cargo build --release
```

- Beim ersten Mal dauert es einige Minuten (alle Abhängigkeiten werden gebaut).
- Ergebnis:
  - **Linux/macOS:** `launcher-native/target/release/dolphinclient-launcher`
  - **Windows:** `launcher-native\target\release\dolphinclient-launcher.exe`

Das ist bereits eine **fertige, doppelklickbare App**. Unter Windows ist die
`.exe` eigenständig; unter macOS/Linux ist es ein normales Binary, das man aus
dem Dateimanager oder Terminal starten kann.

Starten zum Ausprobieren:

```bash
cargo run --release            # oder das Binary direkt starten
```

---

## 3. Den Client bauen

```bash
cd client-rust
cargo build --release
```

- rustup benutzt hier **automatisch Nightly** (durch `rust-toolchain.toml`).
- Der erste Build ist deutlich langsamer (azalea + wgpu sind groß).
- Ergebnis:
  - **Linux/macOS:** `client-rust/target/release/dolphinclient`
  - **Windows:** `client-rust\target\release\dolphinclient.exe`

### Was der Client zum Laufen braucht

Der Client rendert die Welt selbst, braucht aber die **Original-Texturen/-Modelle**
aus dem Vanilla-Client-JAR von Minecraft **26.1** (`client-26.1.jar`). Diese
Datei kommt ausschließlich von Mojang — der **Launcher lädt sie automatisch**
herunter. Startest du den Client von Hand, gibst du sie mit `--mc-jar` an, oder
legst sie unter `.mc-cache/client-26.1.jar` ab (der Client sucht von seinem
Arbeitsverzeichnis aus nach oben danach).

Der Client direkt gestartet (offline, zum Testen gegen einen Testserver):

```bash
cargo run --release -- --server 127.0.0.1:25565 --username Dolphin \
  --mc-jar /pfad/zu/client-26.1.jar
```

Ohne `--server` startet der Client direkt im **Startmenü** (siehe unten).

---

## 4. Das neue Startmenü (wie bei Minecraft)

Der Client startet jetzt — wie das echte Minecraft — mit einem **Titelbildschirm**:

- **Singleplayer** — bewusst **ausgegraut / nicht anwählbar** (DolphinClient ist
  ein reiner Multiplayer-Client: keine Weltgenerierung, kein Speichern).
- **Multiplayer** — Serveradresse eingeben und beitreten (im Offline-Modus auch
  Benutzername). Vom Launcher gestartet ist die Adresse vorausgefüllt.
- **Options…** — FOV, Maus-Empfindlichkeit, Render-Distanz (live einstellbar).
- **Quit Game** — beenden.

Im Spiel öffnet **Esc** ein Minecraft-artiges **Pause-Menü** („Back to Game",
„Options…", „Disconnect").

Menü nur ansehen, ohne Server/Fenster (rendert PNGs, praktisch zum Prüfen):

```bash
cd client-rust
cargo run --release -- --dump-menu ./menushots --mc-jar /pfad/zu/client-26.1.jar
# erzeugt menushots/menu_title.png, menu_multiplayer.png, menu_options.png, menu_pause.png
```

---

## 5. Launcher mit lokalem Client testen

Damit der Launcher **deinen** frisch gebauten Client benutzt (statt ihn von der
Website zu laden), setzt du die Umgebungsvariable `DOLPHIN_CLIENT_BIN` auf den
Pfad deines Client-Binaries und startest dann den Launcher.

**Linux/macOS:**
```bash
export DOLPHIN_CLIENT_BIN="$PWD/client-rust/target/release/dolphinclient"
launcher-native/target/release/dolphinclient-launcher
```

**Windows (PowerShell):**
```powershell
$env:DOLPHIN_CLIENT_BIN = "$PWD\client-rust\target\release\dolphinclient.exe"
.\launcher-native\target\release\dolphinclient-launcher.exe
```

Der Launcher meldet sich an, lädt (falls nötig) das Vanilla-JAR von Mojang und
startet deinen lokalen Client. Ohne diese Variable würde der Launcher den Client
von `https://dolphinclient.de/downloads/` herunterladen — dorthin kommt er
aber nur, wenn du ihn vorher veröffentlichst (Schritt 7).

### 5.1 Offline-Profile, Skin und Cape testen

Im Launcher unter **Accounts** kann ein Offline-Profil angelegt werden. Der Name
wird nach Vanillas Regeln geprüft und erhält dieselbe Offline-UUID wie im Java-
Client. Ein solches Profil kann nur auf Server, die Offline-Mode ausdrücklich
aktiviert haben; es ersetzt keinen Minecraft-Kauf und umgeht Online-Mode nicht.

Unter **Cosmetics** lassen sich pro Profil PNGs importieren:

- Skin: Vanilla 64×64 oder alt 64×32, außerdem ganzzahlige HD-Vielfache;
- Cape: Vanilla 64×32 oder ein HD-Vielfaches;
- Modell: Classic oder Slim.

Der Launcher speichert eine validierte Kopie in seinem Benutzer-Konfigurations-
ordner. Skin und Cape werden nur für den eigenen Spieler gerendert und nie an
den Server gesendet. Direktstart ohne Launcher:

```bash
cargo run --release -- --server 127.0.0.1:25565 --username Dolphin \
  --local-skin /pfad/skin.png --local-cape /pfad/cape.png --skin-model slim \
  --mc-jar /pfad/client-26.1.jar
```

---

## 6. Eine „App" zum Weitergeben paketieren

Die gebauten Binaries sind schon eigenständig. Für die Weitergabe benennst du
sie in die offiziellen Asset-Namen um (die der Launcher/Updater erwartet):

| OS | Launcher-Binary → Asset-Name | Client-Binary → Asset-Name |
|---|---|---|
| **Windows** | `dolphinclient-launcher.exe` → `DolphinClient-windows-x64.exe` | `dolphinclient.exe` → `DolphinClient-Client-windows-x64.exe` |
| **macOS (Apple Silicon)** | `dolphinclient-launcher` → `DolphinClient-macos-arm64` | `dolphinclient` → `DolphinClient-Client-macos-arm64` |
| **macOS (Intel)** | `dolphinclient-launcher` → `DolphinClient-macos-x64` | `dolphinclient` → `DolphinClient-Client-macos-x64` |
| **Linux x64** | `dolphinclient-launcher` → `DolphinClient-linux-x64` | `dolphinclient` → `DolphinClient-Client-linux-x64` |
| **Linux ARM64** | `dolphinclient-launcher` → `DolphinClient-linux-arm64` | `dolphinclient` → `DolphinClient-Client-linux-arm64` |

### 6.1 Windows-Installer (Setup.exe) bauen

Seit 0.6.0 bekommt Windows einen **richtigen Installer** (NSIS): Startmenü- und
Desktop-Verknüpfung, Eintrag in „Apps & Features", Uninstaller — und er ist die
Grundlage für das automatische Selbst-Update des Launchers. Baubar direkt auf
diesem Linux-Server (`sudo apt-get install nsis`, einmalig):

```bash
# nach dem Windows-Cross-Build des Launchers:
makensis -DVERSION=0.6.0 \
  -DBINDIR=launcher-native/target/x86_64-pc-windows-gnu/release \
  -DOUTFILE=/tmp/DolphinClient-Setup-0.6.0.exe \
  deploy/launcher-installer.nsi
```

Veröffentlicht wird er als `DolphinClient-Setup-windows-x64.exe` —
`deploy/publish-windows.sh` erledigt Build + Installer + Publish in einem.
Der Installer installiert **pro Benutzer** (kein Admin nötig) nach
`%LOCALAPPDATA%\Programs\DolphinClient`. Das Selbst-Update lädt künftige
Setups automatisch herunter, prüft die SHA-256 und installiert still (`/S`).

Hinweise:

- **Windows:** Der Setup-Installer ist der Standardweg; die nackte `.exe`
  bleibt als portable Variante verfügbar. Ohne Code-Signatur zeigt SmartScreen
  beim ersten Start eine Warnung („Weitere Informationen" → „Trotzdem ausführen").
- **macOS:** Das Binary ausführbar machen (`chmod +x DolphinClient-macos-arm64`).
  Unsigniert muss man es per Rechtsklick → „Öffnen" bzw. über
  „Systemeinstellungen → Datenschutz & Sicherheit" freigeben.
- **Linux:** `chmod +x` genügt.

---

## 7. Auf der Website veröffentlichen (Downloads aktualisieren)

Damit der öffentliche Launcher deine Binaries findet, müssen sie in den
Download-Ordner der Website und das Manifest neu erzeugt werden. Früher zog das
`update-downloads.sh` aus dem GitHub-Release — **das entfällt jetzt**. Nutze
stattdessen `deploy/publish-local.sh`.

Dieser Rechner **ist** der `dolphinclient.de`-Server. Nach dem lokalen Bauen
(als root):

```bash
# im Repo-Wurzelverzeichnis, nach `cargo build --release` für beide:
deploy/publish-local.sh 0.3.0 \
  launcher-native/target/release/dolphinclient-launcher \
  client-rust/target/release/dolphinclient
```

Das Skript

1. erkennt OS und CPU und kopiert Launcher (+ optional Client) unter den exakten
   Zielnamen nach `/var/www/dolphinclient.de/downloads/`,
2. erzeugt `manifest.json` neu (Größe + SHA-256 für Launcher und Client sowie
   getrennte x64-/ARM64-Ziele),
3. setzt die Rechte auf `www-data`.

> Für Binaries eines **anderen** OS: dort bauen und `publish-local.sh` dort
> ausführen, oder die Datei manuell unter dem passenden Asset-Namen in den
> Download-Ordner legen und danach `node deploy/gen-manifest.mjs <downloads-dir> <version>` laufen lassen.

Website/Backend selbst neu ausrollen (unverändert):

```bash
deploy/redeploy.sh
```

### 7.1 Windows-.exe von Linux aus cross-bauen & veröffentlichen

Auf diesem Server (Linux) werden die **Windows-Binaries direkt cross-kompiliert**
— kein Windows-Rechner nötig. Einmalige Vorbereitung:

```bash
rustup target add x86_64-pc-windows-gnu                        # Launcher (stable)
rustup target add --toolchain nightly x86_64-pc-windows-gnu    # Client (nightly)
sudo apt-get install -y gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64
```

Danach in **einem** Schritt cross-bauen **und** veröffentlichen (als root):

```bash
sudo deploy/publish-windows.sh 0.4.0
```

Das Skript

1. sourct `deploy/win-cross-env.sh` (setzt den mingw-Linker + `CC/CXX/AR`),
2. baut `dolphinclient-launcher.exe` **und** `dolphinclient.exe` für
   `x86_64-pc-windows-gnu`,
3. kopiert sie unter den Asset-Namen `DolphinClient-windows-x64.exe` und
   `DolphinClient-Client-windows-x64.exe` in den Download-Ordner,
4. erzeugt `manifest.json` neu und setzt die Rechte auf `www-data`.

> Nur das erste Cross-Build ist langsam (azalea + wgpu für ein frisches Target).
> Folgende Builds nutzen den Cache und sind schnell — nur die eigenen Crates neu.

---

## 8. Nützliche Zusatzbefehle

```bash
# Tests des Clients (Live-Server-Test überspringt sich ohne laufenden Server):
cd client-rust && cargo test

# Item-Icon-Atlas zum Prüfen als PNG ausgeben:
cargo run --release -- --dump-item-icons icons.png --mc-jar /pfad/zu/client-26.1.jar

# Headless-Render-Rauchtest (braucht einen laufenden Testserver auf 127.0.0.1:25565):
cargo run --release -- --offscreen --server 127.0.0.1:25565 --username Dolphin \
  --out shots --frames 8 --mc-jar /pfad/zu/client-26.1.jar
```

---

## 9. Fehlerbehebung

| Problem | Ursache / Lösung |
|---|---|
| `cargo: command not found` | Terminal neu öffnen oder `source $HOME/.cargo/env`. rustup korrekt installiert? |
| Linker-Fehler unter Windows | Visual Studio Build Tools mit „Desktopentwicklung mit C++" installieren. |
| Linux-Build bricht mit fehlenden `*.h`/`-l…` ab | System-Build-Pakete aus Schritt 1.2 installieren. |
| Client: „no `--mc-jar` given and no `.mc-cache/client-26.1.jar` found" | `--mc-jar /pfad/zu/client-26.1.jar` angeben oder die Datei unter `.mc-cache/` ablegen. Über den Launcher wird sie automatisch geladen. |
| Launcher lädt den alten Client aus dem Netz statt meines lokalen | `DOLPHIN_CLIENT_BIN` auf den Pfad deines Client-Binaries setzen (Schritt 5). |
| Erster Build extrem langsam | Normal — azalea + wgpu sind groß. Folgende Builds nutzen den Cache und sind schnell. |
| `rustfmt`/`clippy` fehlen | Optional: `rustup component add rustfmt clippy` (für den reinen Build nicht nötig). |

---

*Privater Client. Nicht mit Mojang oder Microsoft verbunden. Die Original-Spieldateien
kommen ausschließlich von Mojang und werden nicht selbst gehostet.*
