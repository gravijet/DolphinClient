import { autoUpdater } from "electron-updater";

/**
 * Auto-Update via electron-updater.
 *
 * Der Update-Feed kommt vom DolphinClient-Backend (GET /v1/updates/:channel)
 * und MUSS signiert sein; electron-updater prüft die Signatur der Artefakte.
 * Zusätzlich müssen die Launcher-Binärdateien code-signiert sein
 * (sonst SmartScreen-/Gatekeeper-Warnungen).
 */
export async function checkForUpdates(): Promise<void> {
  autoUpdater.autoDownload = true;

  autoUpdater.on("update-available", (info) =>
    console.log("Update verfügbar:", info.version),
  );
  autoUpdater.on("update-downloaded", () => autoUpdater.quitAndInstall());
  autoUpdater.on("error", (err) => console.error("Updater-Fehler:", err));

  await autoUpdater.checkForUpdates();
}
