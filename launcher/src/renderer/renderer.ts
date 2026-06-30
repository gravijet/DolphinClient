interface MinecraftSession {
  uuid: string;
  username: string;
  accessToken: string;
}

declare global {
  interface Window {
    dolphin: {
      login: () => Promise<MinecraftSession>;
      launch: (session: MinecraftSession) => Promise<void>;
    };
  }
}

const statusEl = document.getElementById("status") as HTMLDivElement;
const loginBtn = document.getElementById("login") as HTMLButtonElement;
const playBtn = document.getElementById("play") as HTMLButtonElement;

let session: MinecraftSession | null = null;

loginBtn.addEventListener("click", async () => {
  statusEl.textContent = "Anmeldung läuft …";
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
