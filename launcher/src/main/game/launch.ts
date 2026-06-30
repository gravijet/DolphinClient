import { MinecraftSession } from "../auth/microsoft";

const VERSION_MANIFEST =
  "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
const TARGET_VERSION = "26.1";

/**
 * Startet Minecraft 26.1 mit Fabric + DolphinClient.
 *
 * Die Original-Spieldateien werden AUSSCHLIESSLICH von Mojang geladen — niemals
 * selbst hosten (Mojang-EULA). Der Nutzer muss das Spiel besitzen.
 *
 * Skelett: Die Schritte sind dokumentiert; Implementierung in Phase 2 (M3).
 */
export async function launchGame(session: MinecraftSession): Promise<void> {
  // 1. version_manifest_v2.json laden, Eintrag für 26.1 finden.        (siehe VERSION_MANIFEST)
  // 2. Versions-JSON laden: Libraries, Asset-Index, Main-Class, JVM-Args.
  // 3. Fehlende Libraries + Assets von Mojang nach .minecraft laden + Hashes prüfen.
  // 4. Fabric Loader 0.18.4 für 26.1 installieren/mergen.
  // 5. DolphinClient-Mod-Jar in den mods-Ordner legen.
  // 6. Classpath bauen, JDK 25 lokalisieren, JVM starten (RAM-Default 4 GB).
  // 7. Auth-Argumente aus session übergeben (uuid, accessToken, username).
  throw new Error(
    `Spielstart noch nicht implementiert (M3) für ${TARGET_VERSION} / ${session.username}. ` +
      `Manifest: ${VERSION_MANIFEST}`,
  );
}
