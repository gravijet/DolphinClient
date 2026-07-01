import { spawn } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { app } from "electron";
import AdmZip from "adm-zip";
import { MinecraftSession } from "../auth/microsoft";

/**
 * Startet Minecraft 26.1 mit Fabric + DolphinClient.
 *
 * Die Original-Spieldateien kommen AUSSCHLIESSLICH von Mojang / dem
 * Fabric-Meta-Service (nie selbst hosten, Mojang-EULA). Der Nutzer muss das
 * Spiel besitzen.
 *
 * Ablauf: Vanilla-Versions-JSON -> Client-JAR + Libraries + Assets laden,
 * Fabric-Profil (Loader-Libraries + mainClass) mergen, Natives extrahieren,
 * Classpath + JVM-/Game-Argumente bauen (Platzhalter ersetzen) und JDK 25
 * starten.
 *
 * Hinweis: Dieser Ablauf ist vollständig implementiert, aber in der
 * Build-Umgebung nicht gegen einen echten Spielstart getestet (kein 26.1 +
 * Account + Grafik). Vor Auslieferung real gegentesten.
 */

const VERSION_MANIFEST =
  "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
const RESOURCES = "https://resources.download.minecraft.net";
const FABRIC_META = "https://meta.fabricmc.net/v2";
const TARGET_VERSION = "26.1";
const LOADER_VERSION = "0.19.3";
// Fabric API von Modrinth nachladen (nicht bündeln — siehe docs/MOD-LICENSES.md).
const FABRIC_API_URL =
  "https://cdn.modrinth.com/data/P7dR8mSH/versions/WC1KT7Yg/fabric-api-0.153.0%2B26.1.2.jar";

type Json = any;

function osName(): "windows" | "osx" | "linux" {
  switch (process.platform) {
    case "win32":
      return "windows";
    case "darwin":
      return "osx";
    default:
      return "linux";
  }
}

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

async function fetchJson(url: string): Promise<Json> {
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

/** Wertet die (optionalen) OS-Regeln einer Library/eines Arguments aus. */
function rulesAllow(rules: Json[] | undefined): boolean {
  if (!rules) return true;
  let allowed = false;
  for (const rule of rules) {
    let match = true;
    if (rule.os?.name) match = rule.os.name === osName();
    if (rule.features) match = false; // Feature-Flags (Demo etc.) hier ignorieren
    if (match) allowed = rule.action === "allow";
  }
  return allowed;
}

/** group:artifact:version -> group/artifact/version/artifact-version.jar */
function mavenPath(name: string): string {
  const [group, artifact, version] = name.split(":");
  return `${group.replace(/\./g, "/")}/${artifact}/${version}/${artifact}-${version}.jar`;
}

function extractNatives(jar: string, nativesDir: string): void {
  const zip = new AdmZip(jar);
  for (const entry of zip.getEntries()) {
    if (entry.isDirectory) continue;
    if (entry.entryName.startsWith("META-INF")) continue;
    zip.extractEntryTo(entry, nativesDir, false, true);
  }
}

async function downloadLibraries(
  libraries: Json[],
  libDir: string,
  nativesDir: string,
): Promise<string[]> {
  const classpath: string[] = [];
  for (const lib of libraries ?? []) {
    if (!rulesAllow(lib.rules)) continue;

    // Vanilla-Stil: downloads.artifact mit URL + Pfad.
    const artifact = lib.downloads?.artifact;
    if (artifact?.url && artifact?.path) {
      const dest = path.join(libDir, artifact.path);
      await downloadFile(artifact.url, dest);
      if (artifact.path.includes("natives-")) {
        extractNatives(dest, nativesDir);
      } else {
        classpath.push(dest);
      }
      continue;
    }

    // Fabric-Stil: name + Maven-Basis-URL.
    if (lib.name && lib.url) {
      const rel = mavenPath(lib.name);
      const dest = path.join(libDir, rel);
      await downloadFile(lib.url.replace(/\/?$/, "/") + rel, dest);
      classpath.push(dest);
    }
  }
  return classpath;
}

async function downloadAssets(assetIndex: Json, root: string): Promise<string> {
  const indexFile = path.join(root, "assets", "indexes", `${assetIndex.id}.json`);
  await downloadFile(assetIndex.url, indexFile);
  const index = JSON.parse(fs.readFileSync(indexFile, "utf8"));
  const objectsDir = path.join(root, "assets", "objects");
  for (const key of Object.keys(index.objects ?? {})) {
    const hash: string = index.objects[key].hash;
    const sub = hash.substring(0, 2);
    await downloadFile(`${RESOURCES}/${sub}/${hash}`, path.join(objectsDir, sub, hash));
  }
  return assetIndex.id;
}

function substitute(arg: string, vars: Record<string, string>): string {
  return arg.replace(/\$\{(\w+)\}/g, (_m, key) => vars[key] ?? "");
}

function collectArgs(section: Json, vars: Record<string, string>): string[] {
  const out: string[] = [];
  if (!Array.isArray(section)) return out;
  for (const arg of section) {
    if (typeof arg === "string") {
      out.push(substitute(arg, vars));
      continue;
    }
    if (!rulesAllow(arg.rules)) continue;
    const values = Array.isArray(arg.value) ? arg.value : [arg.value];
    for (const v of values) out.push(substitute(v, vars));
  }
  return out;
}

/** Verzeichnis mit den mitgelieferten Mod-Jars (Dev vs. paketiert). */
function bundledModsDir(): string {
  return app.isPackaged
    ? path.join(process.resourcesPath, "mods")
    : path.join(__dirname, "..", "..", "..", "resources", "mods");
}

/** Legt Fabric API (Modrinth) + die gebündelte DolphinClient-Mod in mods/. */
async function installMods(root: string): Promise<void> {
  const modsDir = path.join(root, "mods");
  fs.mkdirSync(modsDir, { recursive: true });

  await downloadFile(FABRIC_API_URL, path.join(modsDir, "fabric-api-0.153.0+26.1.2.jar"));

  const src = bundledModsDir();
  if (fs.existsSync(src)) {
    for (const file of fs.readdirSync(src)) {
      if (file.endsWith(".jar")) {
        fs.copyFileSync(path.join(src, file), path.join(modsDir, file));
      }
    }
  }
}

async function getVanillaVersion(): Promise<Json> {
  const manifest = await fetchJson(VERSION_MANIFEST);
  const entry = manifest.versions.find((v: Json) => v.id === TARGET_VERSION);
  if (!entry) throw new Error(`Version ${TARGET_VERSION} nicht im Manifest gefunden.`);
  return fetchJson(entry.url);
}

export async function launchGame(session: MinecraftSession): Promise<void> {
  const root = minecraftDir();
  const libDir = path.join(root, "libraries");
  const nativesDir = path.join(root, "versions", TARGET_VERSION, "natives");
  fs.mkdirSync(nativesDir, { recursive: true });

  // 1. Vanilla-Versions-JSON + Client-JAR.
  const version = await getVanillaVersion();
  const clientJar = path.join(root, "versions", TARGET_VERSION, `${TARGET_VERSION}.jar`);
  await downloadFile(version.downloads.client.url, clientJar);

  // 2. Vanilla-Libraries (+ Natives) -> Classpath.
  const classpath = await downloadLibraries(version.libraries, libDir, nativesDir);
  classpath.push(clientJar);

  // 3. Assets (Index + Objekte).
  const assetIndexId = await downloadAssets(version.assetIndex, root);

  // 4. Fabric-Profil: Loader-Libraries + mainClass + evtl. zusätzliche Args.
  const fabric = await fetchJson(
    `${FABRIC_META}/versions/loader/${TARGET_VERSION}/${LOADER_VERSION}/profile/json`,
  );
  for (const cp of await downloadLibraries(fabric.libraries, libDir, nativesDir)) {
    if (!classpath.includes(cp)) classpath.push(cp);
  }
  const mainClass: string = fabric.mainClass ?? version.mainClass;

  // 5. Fabric API (Modrinth) + gebündelte DolphinClient-Mod nach mods/ legen.
  await installMods(root);

  // 6. Argumente bauen (Platzhalter ersetzen).
  const vars: Record<string, string> = {
    auth_player_name: session.username,
    version_name: TARGET_VERSION,
    game_directory: root,
    assets_root: path.join(root, "assets"),
    assets_index_name: assetIndexId,
    auth_uuid: session.uuid,
    auth_access_token: session.accessToken,
    clientid: "",
    auth_xuid: "",
    user_type: "msa",
    version_type: "release",
    natives_directory: nativesDir,
    launcher_name: "DolphinClient",
    launcher_version: "0.1.0",
    classpath: classpath.join(path.delimiter),
  };

  const jvmArgs = [
    ...collectArgs(version.arguments?.jvm, vars),
    ...collectArgs(fabric.arguments?.jvm, vars),
    `-Xmx${process.env.DOLPHIN_RAM ?? "4G"}`,
  ];
  // Fallback für das alte Argument-Format (26.1 nutzt aber `arguments`).
  if (!version.arguments) {
    jvmArgs.push(`-Djava.library.path=${nativesDir}`, "-cp", vars.classpath);
  }
  const gameArgs = collectArgs(version.arguments?.game, vars);

  // 7. JDK 25 starten.
  const javaBin = process.env.DOLPHIN_JDK25 ?? "java";
  const child = spawn(javaBin, [...jvmArgs, mainClass, ...gameArgs], {
    cwd: root,
    stdio: "inherit",
  });
  child.on("error", (err) => console.error("JVM-Start fehlgeschlagen:", err));
}
