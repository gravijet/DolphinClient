import { spawn } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { MinecraftSession } from "../auth/microsoft";

/**
 * Startet Minecraft 26.1. Die Original-Spieldateien kommen AUSSCHLIESSLICH von
 * Mojang (nie selbst hosten, Mojang-EULA).
 *
 * Stand M3: Manifest + Versions-JSON werden geladen, Client-JAR + Libraries
 * heruntergeladen, Classpath gebaut und die JVM gestartet. Noch offen (klar
 * markiert): vollständige Asset-Objekte, Natives-Extraktion, Fabric-Loader-Merge
 * und das Einlegen der DolphinClient-Mod.
 */

const VERSION_MANIFEST =
  "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
const TARGET_VERSION = "26.1";

export function minecraftDir(): string {
  const home = os.homedir();
  switch (process.platform) {
    case "win32":
      return path.join(
        process.env.APPDATA ?? path.join(home, "AppData", "Roaming"),
        ".minecraft",
      );
    case "darwin":
      return path.join(home, "Library", "Application Support", "minecraft");
    default:
      return path.join(home, ".minecraft");
  }
}

async function fetchJson(url: string): Promise<any> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url} -> HTTP ${res.status}`);
  return res.json();
}

async function downloadFile(url: string, dest: string): Promise<void> {
  if (fs.existsSync(dest)) return;
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  const res = await fetch(url);
  if (!res.ok) throw new Error(`Download fehlgeschlagen: ${url} (HTTP ${res.status})`);
  fs.writeFileSync(dest, Buffer.from(await res.arrayBuffer()));
}

export async function getVersionJson(): Promise<any> {
  const manifest = await fetchJson(VERSION_MANIFEST);
  const entry = manifest.versions.find((v: any) => v.id === TARGET_VERSION);
  if (!entry) throw new Error(`Version ${TARGET_VERSION} nicht im Manifest gefunden.`);
  return fetchJson(entry.url);
}

export async function launchGame(session: MinecraftSession): Promise<void> {
  const root = minecraftDir();
  const version = await getVersionJson();

  // 1. Client-JAR laden.
  const clientJar = path.join(root, "versions", TARGET_VERSION, `${TARGET_VERSION}.jar`);
  await downloadFile(version.downloads.client.url, clientJar);

  // 2. Libraries laden + Classpath bauen.
  //    TODO: OS-/Feature-Regeln (lib.rules) vollständig auswerten.
  const libDir = path.join(root, "libraries");
  const classpath: string[] = [clientJar];
  for (const lib of version.libraries ?? []) {
    const artifact = lib.downloads?.artifact;
    if (!artifact?.url) continue;
    const dest = path.join(libDir, artifact.path);
    await downloadFile(artifact.url, dest);
    classpath.push(dest);
  }

  // 3. TODO(M3): Asset-Index + alle Objekte nach assets/objects laden.
  // 4. TODO(M3): Natives extrahieren.
  // 5. TODO(M3): Fabric Loader 0.18.4 für 26.1 mergen (eigene mainClass + libs)
  //              und die DolphinClient-Mod nach mods/ legen.

  // 6. JVM starten (JDK 25 erwartet; RAM-Default 4 GB).
  const javaBin = process.env.DOLPHIN_JDK25 ?? "java";
  const args = [
    `-Xmx${process.env.DOLPHIN_RAM ?? "4G"}`,
    "-cp",
    classpath.join(path.delimiter),
    version.mainClass,
    "--username",
    session.username,
    "--uuid",
    session.uuid,
    "--accessToken",
    session.accessToken,
    "--version",
    TARGET_VERSION,
    "--gameDir",
    root,
    "--assetsDir",
    path.join(root, "assets"),
  ];

  const child = spawn(javaBin, args, { cwd: root, stdio: "inherit" });
  child.on("error", (err) => console.error("JVM-Start fehlgeschlagen:", err));
}
