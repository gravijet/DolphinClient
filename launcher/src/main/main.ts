import { app, BrowserWindow, ipcMain } from "electron";
import * as path from "path";
import { startMicrosoftLogin } from "./auth/microsoft";
import { launchGame } from "./game/launch";
import { checkForUpdates } from "./updater";

let mainWindow: BrowserWindow | null = null;

function createWindow(): void {
  mainWindow = new BrowserWindow({
    width: 1000,
    height: 640,
    resizable: false,
    title: "DolphinClient",
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  // Hinweis: Der Build muss die Renderer-Assets nach dist/renderer kopieren
  // (index.html, css). Siehe launcher/README.md.
  mainWindow.loadFile(path.join(__dirname, "..", "renderer", "index.html"));
}

app.whenReady().then(() => {
  createWindow();
  checkForUpdates().catch((e) => console.error("Update-Check fehlgeschlagen", e));

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow();
    }
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    app.quit();
  }
});

// IPC-Handler, vom Renderer über das Preload-API aufgerufen.
ipcMain.handle("auth:login", async (event) =>
  // Den Device-Code (Link-Code) an den Renderer durchreichen, damit er ihn anzeigt.
  startMicrosoftLogin((prompt) => event.sender.send("auth:prompt", prompt)),
);
ipcMain.handle("game:launch", async (_event, session) => launchGame(session));
