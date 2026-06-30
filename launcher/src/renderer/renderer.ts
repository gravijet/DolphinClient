interface MinecraftSession {
  uuid: string;
  username: string;
  accessToken: string;
}

interface DeviceCodePrompt {
  userCode: string;
  verificationUri: string;
  message: string;
}

declare global {
  interface Window {
    dolphin: {
      login: () => Promise<MinecraftSession>;
      launch: (session: MinecraftSession) => Promise<void>;
      onAuthPrompt: (callback: (prompt: DeviceCodePrompt) => void) => void;
    };
  }
}

const statusEl = document.getElementById("status") as HTMLDivElement;
const loginBtn = document.getElementById("login") as HTMLButtonElement;
const playBtn = document.getElementById("play") as HTMLButtonElement;

let session: MinecraftSession | null = null;

// Device-Code (Link-Code) anzeigen, sobald der Hauptprozess ihn liefert.
window.dolphin.onAuthPrompt((prompt) => {
  statusEl.innerHTML =
    `Öffne <b>${prompt.verificationUri}</b> und gib den Code ein:<br>` +
    `<span style="font-size:1.6rem;letter-spacing:3px">${prompt.userCode}</span>`;
});

loginBtn.addEventListener("click", async () => {
  statusEl.textContent = "Anmeldung wird vorbereitet …";
  try {
    session = await window.dolphin.login();
    statusEl.textContent = `Angemeldet als ${session.username}`;
    playBtn.disabled = false;
  } catch (e) {
    statusEl.textContent = "Login fehlgeschlagen: " + (e as Error).message;
  }
});

playBtn.addEventListener("click", async () => {
  if (!session) return;
  statusEl.textContent = "Starte Minecraft 26.1 …";
  try {
    await window.dolphin.launch(session);
  } catch (e) {
    statusEl.textContent = "Start fehlgeschlagen: " + (e as Error).message;
  }
});

export {};
