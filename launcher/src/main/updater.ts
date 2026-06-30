import { autoUpdater } from "electron-updater";

/**
 * Auto-Update via electron-updater (generischer Provider).
 *
 * Der Feed liegt beim DolphinClient-Backend; electron-updater lädt dort die
 * `latest.yml` (von electron-builder beim Release erzeugt) und prüft die
 * Signatur/Hashes der Artefakte. Zusätzlich müssen die Launcher-Binärdateien
 * code-signiert sein (sonst SmartScreen/Gatekeeper).
 *
 * In der Entwicklung (ohne DOLPHIN_UPDATE_URL) wird der Check übersprungen.
 */
export async function checkForUpdates(): Promise<void> {
  const feedUrl = process.env.DOLPHIN_UPDATE_URL;
  if (!feedUrl) {
    console.log("DOLPHIN_UPDATE_URL nicht gesetzt — Auto-Update übersprungen (Dev).");
    return;
  }

  autoUpdater.setFeedURL({ provider: "generic", url: feedUrl });
  autoUpdater.autoDownload = true;

  autoUpdater.on("update-available", (info) =>
    console.log("Update verfügbar:", info.version),
  );
  autoUpdater.on("update-not-available", () => console.log("Launcher ist aktuell."));
  autoUpdater.on("update-downloaded", () => autoUpdater.quitAndInstall());
  autoUpdater.on("error", (err) => console.error("Updater-Fehler:", err));

  await autoUpdater.checkForUpdates();
}
