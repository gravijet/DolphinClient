# DolphinClient — Launcher (`launcher/`)

Desktop-Launcher auf Basis **Electron + TypeScript**. Aufgaben: Microsoft-Login,
Spielstart von Minecraft 26.1 (Fabric + DolphinClient-Mod) und Auto-Update.

## Entwicklung

```bash
npm install          # im Repo-Root (Workspaces)
npm run dev --workspace launcher
```

> Der Build kompiliert nur `.ts`. Die Renderer-Assets (`index.html`, CSS) müssen
> nach `dist/renderer` kopiert werden — dafür später ein Copy-Skript oder ein
> Bundler (z. B. electron-vite) ergänzen.

## Struktur

```
src/main/main.ts          Electron-Hauptprozess + Fenster + IPC
src/main/preload.ts       Sichere Renderer-Brücke (contextBridge)
src/main/auth/microsoft.ts Microsoft-OAuth (Device-Code-Flow) -> MC-Session
src/main/game/launch.ts   Mojang-Dateien laden + JDK 25 starten
src/main/updater.ts       Auto-Update (electron-updater, signiert)
src/renderer/index.html   UI
src/renderer/renderer.ts  UI-Logik
```

## Wichtige Hinweise

- **Nur legitimer Microsoft-Login**, keine Cracked-Accounts (Mojang-EULA).
- Spieldateien **immer von Mojang** laden, nie selbst hosten.
- Tokens in die OS-Keychain (Electron `safeStorage` / keytar).
- Vor Release: **Code-Signing** für Windows/macOS (sonst SmartScreen/Gatekeeper).
- `DOLPHIN_MS_CLIENT_ID` als Env-Var setzen (Azure-App-Registrierung).
