import { app, safeStorage } from "electron";
import * as fs from "node:fs";
import * as path from "node:path";

/**
 * Sichere Ablage des Refresh-Tokens via Electron safeStorage (nutzt die
 * OS-Keychain/DPAPI). Niemals im Klartext speichern.
 */

function file(): string {
  return path.join(app.getPath("userData"), "auth.bin");
}

export function saveRefreshToken(token: string): void {
  if (!safeStorage.isEncryptionAvailable()) {
    console.warn("safeStorage nicht verfügbar — Refresh-Token wird nicht persistiert.");
    return;
  }
  fs.writeFileSync(file(), safeStorage.encryptString(token));
}

export function loadRefreshToken(): string | null {
  try {
    return safeStorage.decryptString(fs.readFileSync(file()));
  } catch {
    return null;
  }
}

export function clearTokens(): void {
  try {
    fs.unlinkSync(file());
  } catch {
    /* nichts zu löschen */
  }
}
