# DolphinClient

Ein **nativer Minecraft-Client für 26.1**, komplett in Rust geschrieben —
eigener Renderer (wgpu), eigenes Protokoll (azalea), echte Mojang-Texturen und
-Sounds. Dazu ein schlanker nativer Launcher und die Website mit Dashboard.
Kein Java, kein Fabric, kein Singleplayer: DolphinClient verbindet 1:1 mit
echten 26.1-Servern.

Web: **[example.invalid](https://example.invalid)**

## Aufbau

Das Repo besteht aus genau drei Komponenten:

| Ordner | Zweck | Tech |
|---|---|---|
| `client-rust/`     | **Der Client** — der eigentliche native Minecraft-26.1-Client | Rust (nightly), wgpu + azalea |
| `launcher-native/` | **Der Launcher** — Login, Client-Download, Auto-Update, Dashboard-Bridge | Rust (stable), eframe/egui |
| `website/`         | **Die Website** — Marketing, Download, Live-Dashboard | Next.js (statischer Export) |

`deploy/` enthält die Server-/Build-Skripte für Windows, Linux und macOS
(architekturspezifisches Manifest, verifizierte Updates, nginx-Publish). Die vollständige Build-Anleitung steht in
[`ANLEITUNG-BUILD.md`](ANLEITUNG-BUILD.md).

## Profile und private Cosmetics

Der Launcher verwaltet Microsoft-Konten und Vanilla-kompatible Offline-Profile.
Offline-Profile funktionieren ausschließlich auf Servern, die Offline-Mode
bewusst erlauben; sie umgehen keine Microsoft-/Mojang-Prüfung. Pro Profil kann
ein lokaler Skin, ein lokales Cape und das Classic-/Slim-Modell gewählt werden.
Diese Dateien werden nur im eigenen Client gerendert und nie hochgeladen.

## Schnellstart

```bash
# Launcher bauen/starten (Rust stable)
cd launcher-native && cargo run            # bzw. cargo build --release

# Client bauen (Rust nightly wird per rust-toolchain.toml automatisch gewählt)
cd client-rust && cargo build --release

# Website lokal starten
npm install
npm run dev:website
```

Der Launcher lädt den Client normalerweise von der Website nach; zum lokalen
Testen zeigt man ihn per `DOLPHIN_CLIENT_BIN=<pfad>` auf den lokal gebauten
Client (siehe [`ANLEITUNG-BUILD.md`](ANLEITUNG-BUILD.md)).
