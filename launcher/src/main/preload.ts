import { contextBridge, ipcRenderer } from "electron";

interface DeviceCodePrompt {
  userCode: string;
  verificationUri: string;
  message: string;
}

/**
 * Sichere Brücke zwischen Renderer und Hauptprozess (contextIsolation an,
 * nodeIntegration aus). Der Renderer bekommt nur diese schmale API.
 */
contextBridge.exposeInMainWorld("dolphin", {
  login: () => ipcRenderer.invoke("auth:login"),
  launch: (session: unknown) => ipcRenderer.invoke("game:launch", session),
  onAuthPrompt: (callback: (prompt: DeviceCodePrompt) => void) =>
    ipcRenderer.on("auth:prompt", (_event, prompt) => callback(prompt)),
});
