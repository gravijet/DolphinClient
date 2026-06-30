import { setTimeout as sleep } from "node:timers/promises";
import { saveRefreshToken } from "./tokenStore";

/**
 * Microsoft-Login über den OAuth-2.0-Device-Code-Flow ("Link-Code"):
 * Der Nutzer öffnet eine URL, tippt einen kurzen Code ein — kein eingebetteter
 * Browser, kein Redirect-Handling nötig. Danach die Token-Kette bis zur
 * Minecraft-Session.
 *
 * WICHTIG: Nur legitimer Microsoft-Login. KEINE Cracked-Accounts (Mojang-EULA).
 * Benötigt EINE (einmalig vom Entwickler registrierte) Azure-App-Client-ID in
 * DOLPHIN_MS_CLIENT_ID — Endnutzer sehen Azure nie.
 */

export interface MinecraftSession {
  uuid: string;
  username: string;
  accessToken: string;
}

export interface DeviceCodePrompt {
  userCode: string;
  verificationUri: string;
  message: string;
}

const TENANT = "consumers";
const DEVICECODE_URL = `https://login.microsoftonline.com/${TENANT}/oauth2/v2.0/devicecode`;
const TOKEN_URL = `https://login.microsoftonline.com/${TENANT}/oauth2/v2.0/token`;
const SCOPE = "XboxLive.signin offline_access";
const DEVICE_GRANT = "urn:ietf:params:oauth:grant-type:device_code";

function clientId(): string {
  const id = process.env.DOLPHIN_MS_CLIENT_ID;
  if (!id) {
    throw new Error(
      "DOLPHIN_MS_CLIENT_ID ist nicht gesetzt. Eine Azure-App registrieren " +
        "(Personal accounts, 'Allow public client flows' = Yes) und die " +
        "Application (client) ID als Umgebungsvariable setzen.",
    );
  }
  return id;
}

async function postForm(url: string, fields: Record<string, string>): Promise<any> {
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams(fields),
  });
  return res.json();
}

async function postJson(url: string, body: unknown, bearer?: string): Promise<any> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    Accept: "application/json",
  };
  if (bearer) headers.Authorization = `Bearer ${bearer}`;
  const res = await fetch(url, { method: "POST", headers, body: JSON.stringify(body) });
  if (!res.ok) throw new Error(`${url} -> HTTP ${res.status}`);
  return res.json();
}

export async function loginWithDeviceCode(
  onPrompt: (prompt: DeviceCodePrompt) => void,
): Promise<MinecraftSession> {
  const id = clientId();

  // 1. Device-Code anfordern und dem Nutzer Code + URL zeigen.
  const dc = await postForm(DEVICECODE_URL, { client_id: id, scope: SCOPE });
  if (dc.error) throw new Error(`devicecode: ${dc.error_description ?? dc.error}`);
  onPrompt({
    userCode: dc.user_code,
    verificationUri: dc.verification_uri,
    message: dc.message,
  });

  // 2. Token-Endpoint pollen, bis der Nutzer bestätigt hat.
  let interval = (dc.interval ?? 5) as number;
  const deadline = Date.now() + (dc.expires_in ?? 900) * 1000;
  let msToken: string | undefined;
  while (Date.now() < deadline) {
    await sleep(interval * 1000);
    const tok = await postForm(TOKEN_URL, {
      grant_type: DEVICE_GRANT,
      client_id: id,
      device_code: dc.device_code,
    });
    if (tok.error === "authorization_pending") continue;
    if (tok.error === "slow_down") {
      interval += 5;
      continue;
    }
    if (tok.error) throw new Error(`token: ${tok.error_description ?? tok.error}`);
    msToken = tok.access_token;
    if (tok.refresh_token) saveRefreshToken(tok.refresh_token);
    break;
  }
  if (!msToken) throw new Error("Anmeldung abgelaufen — bitte erneut versuchen.");

  // 3. Xbox-Live-Authentifizierung.
  const xbl = await postJson("https://user.auth.xboxlive.com/user/authenticate", {
    Properties: {
      AuthMethod: "RPS",
      SiteName: "user.auth.xboxlive.com",
      RpsTicket: `d=${msToken}`,
    },
    RelyingParty: "http://auth.xboxlive.com",
    TokenType: "JWT",
  });

  // 4. XSTS-Token.
  const xsts = await postJson("https://xsts.auth.xboxlive.com/xsts/authorize", {
    Properties: { SandboxId: "RETAIL", UserTokens: [xbl.Token] },
    RelyingParty: "rp://api.minecraftservices.com/",
    TokenType: "JWT",
  });
  const uhs = xsts.DisplayClaims.xui[0].uhs;

  // 5. Bei den Minecraft-Services anmelden.
  const mc = await postJson(
    "https://api.minecraftservices.com/authentication/login_with_xbox",
    { identityToken: `XBL3.0 x=${uhs};${xsts.Token}` },
  );
  const accessToken: string = mc.access_token;

  // 6. Profil (UUID + Name) abrufen.
  const profRes = await fetch("https://api.minecraftservices.com/minecraft/profile", {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  if (!profRes.ok) {
    throw new Error(
      `Minecraft-Profil nicht abrufbar (HTTP ${profRes.status}). Besitzt das Konto ` +
        `Minecraft? Ggf. ist die client_id noch nicht für die Minecraft-API freigeschaltet.`,
    );
  }
  const profile = await profRes.json();
  return { uuid: profile.id, username: profile.name, accessToken };
}

/** Vom Hauptprozess aufgerufen (IPC-Handler). */
export function startMicrosoftLogin(
  onPrompt: (prompt: DeviceCodePrompt) => void,
): Promise<MinecraftSession> {
  return loginWithDeviceCode(onPrompt);
}
