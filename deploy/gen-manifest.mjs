#!/usr/bin/env node
// Erzeugt downloads/manifest.json aus den vorhandenen nativen Launcher-Binaries.
// Aufruf: node gen-manifest.mjs [downloadsDir] [version]
import { readdirSync, statSync, readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { join } from "node:path";

const dir = process.argv[2] || "/var/www/example.invalid/downloads";
const version = process.argv[3] || process.env.VERSION || "0.2.0";

// Native Launcher-Binaries aus dem GitHub-Release:
//   DolphinClient-windows-x64.exe · DolphinClient-macos-arm64 · DolphinClient-linux-x64
const MATCHERS = [
  { os: "windows", label: "Windows", ext: "exe", re: /windows.*\.exe$/i },
  { os: "macos", label: "macOS", ext: "bin", re: /macos/i },
  { os: "linux", label: "Linux", ext: "bin", re: /linux/i },
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

const platforms = {};
for (const m of MATCHERS) {
  const found = files.find(
    (f) => m.re.test(f) && statSync(join(dir, f)).isFile(),
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

const manifest = {
  product: "DolphinClient",
  minecraft: "26.1",
  version,
  generatedAt: new Date().toISOString(),
  platforms,
};

writeFileSync(join(dir, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
const ready = Object.values(platforms).filter((p) => p.available).length;
console.log(`manifest.json geschrieben (${ready}/3 Plattformen, Version ${manifest.version}).`);
