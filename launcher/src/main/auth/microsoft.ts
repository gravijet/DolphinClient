/**
 * Microsoft-Login (OAuth 2.0 Device-Code-Flow) -> Xbox Live -> Minecraft Services.
 *
 * WICHTIG: Ausschließlich legitimer Microsoft-Login. KEINE Cracked-Accounts —
 * das verstößt gegen die Mojang-EULA und ist nicht verhandelbar.
 *
 * Skelett: Die vollständige Token-Kette ist als klare Schrittfolge angelegt;
 * die echten HTTP-Aufrufe und die sichere Token-Speicherung (OS-Keychain via
 * Electron safeStorage / keytar) folgen in Phase 2 (M3).
 */

export interface MinecraftSession {
  uuid: string;
  username: string;
  accessToken: string;
}

const MS_CLIENT_ID = process.env.DOLPHIN_MS_CLIENT_ID ?? "<azure-app-client-id>";
const SCOPE = "XboxLive.signin offline_access";

export async function startMicrosoftLogin(): Promise<MinecraftSession> {
  // 1. Device-Code anfordern:
  //    POST https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode
  // 2. Nutzer gibt den Code auf https://microsoft.com/link ein; währenddessen
  //    den Token-Endpoint pollen, bis ein MS-Access-Token vorliegt.
  // 3. Xbox-Live-Auth:    POST https://user.auth.xboxlive.com/user/authenticate
  // 4. XSTS-Token:        POST https://xsts.auth.xboxlive.com/xsts/authorize
  // 5. Minecraft-Login:   POST https://api.minecraftservices.com/authentication/login_with_xbox
  // 6. Besitz + Profil:   GET  https://api.minecraftservices.com/minecraft/profile
  // 7. accessToken sicher in der OS-Keychain ablegen; Refresh-Token rotieren.
  throw new Error(
    `Microsoft-Login noch nicht implementiert (M3). Client-ID=${MS_CLIENT_ID}, Scope=${SCOPE}`,
  );
}
