import { contextBridge, ipcRenderer } from "electron";

/**
 * Sichere Brücke zwischen Renderer und Hauptprozess (contextIsolation an,
 * nodeIntegration aus). Der Renderer bekommt nur diese schmale API.
 */
contextBridge.exposeInMainWorld("dolphin", {
  login: () => ipcRenderer.invoke("auth:login"),
  launch: (session: unknown) => ipcRenderer.invoke("game:launch", session),
});
