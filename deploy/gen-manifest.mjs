#!/usr/bin/env node
// Erzeugt downloads/manifest.json aus den vorhandenen nativen Binaries.
// Aufruf: node gen-manifest.mjs [downloadsDir] [version] [minecraftVersion]
//
// Das Manifest beschreibt ZWEI Dinge:
//   1. `platforms` — der Launcher (das, was Nutzer selbst herunterladen).
//   2. `client`    — die native Client-Binary, die der Launcher NACHLÄDT.
//
// Der Launcher vergleicht die `sha256` aus `client.<os>` mit seiner lokal
// gecachten Client-Binary und lädt neu, sobald sie sich unterscheidet — so
// bekommt jeder automatisch die neueste Client-Version, ohne den Launcher neu
// installieren zu müssen.
import { readdirSync, statSync, readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { join } from "node:path";

const dir = process.argv[2] || "/var/www/dolphin.gravijet.net/downloads";
const version = process.argv[3] || process.env.VERSION || "0.2.0";
const minecraft = process.argv[4] || process.env.MINECRAFT || "26.1";

// Launcher-Binaries (das -Client- ausschliessen) …
const isLauncher = (f) => !/-Client-/i.test(f);
const LAUNCHER_MATCHERS = [
  { os: "windows", label: "Windows", ext: "exe", re: /windows.*\.exe$/i },
  { os: "macos", label: "macOS", ext: "bin", re: /macos/i },
  { os: "linux", label: "Linux", ext: "bin", re: /linux/i },
];
// … und die native Client-Binaries (nur -Client-).
const CLIENT_MATCHERS = [
  { os: "windows", re: /-Client-windows.*\.exe$/i },
  { os: "macos", re: /-Client-macos/i },
  { os: "linux", re: /-Client-linux/i },
];

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

let files = [];
try {
  files = readdirSync(dir);
} catch {
  files = [];
}

// Launcher-Plattformen (mit Verfügbarkeits-Flag für die Website).
const platforms = {};
for (const m of LAUNCHER_MATCHERS) {
  const found = files.find(
    (f) => m.re.test(f) && isLauncher(f) && statSync(join(dir, f)).isFile(),
  );
  if (found) {
    const full = join(dir, found);
    platforms[m.os] = {
      available: true,
      label: m.label,
      ext: m.ext,
      file: found,
      url: `/downloads/${found}`,
      size: statSync(full).size,
      sha256: sha256(full),
    };
  } else {
    platforms[m.os] = { available: false, label: m.label, ext: m.ext };
  }
}

// Client-Binaries: version + sha256 pro OS, damit der Launcher gezielt
// nachladen kann, wenn sich die veröffentlichte Version ändert.
const client = { version };
for (const m of CLIENT_MATCHERS) {
  const found = files.find(
    (f) => m.re.test(f) && statSync(join(dir, f)).isFile(),
  );
  if (found) {
    const full = join(dir, found);
    client[m.os] = {
      file: found,
      url: `/downloads/${found}`,
      size: statSync(full).size,
      sha256: sha256(full),
    };
  }
}

const manifest = {
  product: "DolphinClient",
  minecraft,
  version,
  generatedAt: new Date().toISOString(),
  platforms,
  client,
};

writeFileSync(join(dir, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
const ready = Object.values(platforms).filter((p) => p.available).length;
const clientReady = CLIENT_MATCHERS.filter((m) => client[m.os]).length;
console.log(
  `manifest.json geschrieben (Launcher ${ready}/3, Client ${clientReady}/3, ` +
    `Version ${manifest.version}, Minecraft ${minecraft}).`,
);
