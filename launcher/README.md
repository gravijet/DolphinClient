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
src/main/main.ts            Electron-Hauptprozess + Fenster + IPC
src/main/preload.ts         Sichere Renderer-Brücke (contextBridge)
src/main/auth/microsoft.ts  Microsoft-Device-Code-Flow -> Minecraft-Session
src/main/auth/tokenStore.ts Refresh-Token sicher (Electron safeStorage)
src/main/game/launch.ts     Mojang-Dateien laden + JDK 25 starten
src/main/updater.ts         Auto-Update (electron-updater, signiert)
src/renderer/index.html     UI
src/renderer/renderer.ts    UI-Logik (zeigt den Link-Code an)
```

## Login (M3)

Device-Code-Flow ("Link-Code"): Beim Klick auf „Anmelden" zeigt der Launcher
einen kurzen Code + eine URL. Der Nutzer öffnet die URL, tippt den Code ein,
bestätigt — fertig. Voraussetzung: `DOLPHIN_MS_CLIENT_ID` ist gesetzt
(siehe `.env.example`).

Der Spielstart (`game/launch.ts`) ist **vollständig implementiert**: Vanilla-
Versions-JSON, Client-JAR, Libraries (mit OS-Regeln), Assets (Index + Objekte),
Natives-Extraktion, **Fabric-Profil-Merge** (Loader-Libraries + mainClass),
Classpath- und JVM-/Game-Argument-Bau (Platzhalter-Ersetzung), JDK-25-Start.

Der Launcher installiert beim Start automatisch **Fabric API** (von Modrinth
nachgeladen, nicht gebündelt) und die **gebündelte DolphinClient-Mod**
(`resources/mods/`, via electron-builder als `extraResources` paketiert) nach
`.minecraft/mods/`.

> **Runtime nicht getestet:** In der Build-Umgebung gibt es kein 26.1 + Account +
> Grafik. Der komplette Flow ist implementiert und typgeprüft, aber vor
> Auslieferung real gegenzutesten.

## Wichtige Hinweise

- **Nur legitimer Microsoft-Login**, keine Cracked-Accounts (Mojang-EULA).
- Spieldateien **immer von Mojang** laden, nie selbst hosten.
- Tokens in die OS-Keychain (Electron `safeStorage` / keytar).
- Vor Release: **Code-Signing** für Windows/macOS (sonst SmartScreen/Gatekeeper).
- `DOLPHIN_MS_CLIENT_ID` als Env-Var setzen (Azure-App-Registrierung).
