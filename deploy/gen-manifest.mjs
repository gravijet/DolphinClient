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

const dir = process.argv[2] || "/var/www/dolphinclient.de/downloads";
const version = process.argv[3] || process.env.VERSION || "0.2.0";
const minecraft = process.argv[4] || process.env.MINECRAFT || "26.1";

// Every downloadable binary is architecture-specific. Broad `platforms` and
// `client.<os>` aliases remain below for old launchers/the website, while new
// launchers resolve the exact target and can never install an ARM build on an
// Intel Mac (or vice versa).
const TARGETS = [
  { key: "windows-x64", os: "windows", label: "Windows", arch: "x64", ext: "exe",
    launcher: /setup.*windows-x64.*\.exe$/i, launcherFallback: /^DolphinClient-windows-x64\.exe$/i,
    client: /-Client-windows-x64\.exe$/i },
  { key: "macos-arm64", os: "macos", label: "macOS", arch: "Apple Silicon", ext: "bin",
    launcher: /^DolphinClient-macos-arm64$/i, client: /-Client-macos-arm64$/i },
  { key: "macos-x64", os: "macos", label: "macOS", arch: "Intel", ext: "bin",
    launcher: /^DolphinClient-macos-x64$/i, client: /-Client-macos-x64$/i },
  { key: "linux-x64", os: "linux", label: "Linux", arch: "x64", ext: "bin",
    launcher: /^DolphinClient-linux-x64$/i, client: /-Client-linux-x64$/i },
  { key: "linux-arm64", os: "linux", label: "Linux", arch: "ARM64", ext: "bin",
    launcher: /^DolphinClient-linux-arm64$/i, client: /-Client-linux-arm64$/i },
];
const PREFERRED = {
  windows: "windows-x64",
  macos: "macos-arm64",
  linux: "linux-x64",
};

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

let files = [];
try {
  files = readdirSync(dir);
} catch {
  files = [];
}

const launcherTargets = {};
for (const target of TARGETS) {
  const found =
    files.find((f) => target.launcher.test(f) && statSync(join(dir, f)).isFile()) ??
    (target.launcherFallback
      ? files.find((f) => target.launcherFallback.test(f) && statSync(join(dir, f)).isFile())
      : undefined);
  if (found) {
    const full = join(dir, found);
    launcherTargets[target.key] = {
      available: true,
      label: target.label,
      ext: target.ext,
      target: target.key,
      arch: target.arch,
      file: found,
      // Cache-bust the fixed filename: the launcher installer/binary keep the
      // same URL every release, so Cloudflare would otherwise serve the previous
      // version from its edge cache (breaking the self-update SHA-256 check and
      // handing website visitors a stale download). A ?v=<version> query makes
      // each release a fresh cache key; manifest.json itself is served no-store.
      url: `/downloads/${found}?v=${version}`,
      size: statSync(full).size,
      sha256: sha256(full),
    };
  }
}

// Website/old-launcher view: one preferred artifact per OS, falling back to
// the other architecture if only that build has been published so availability
// stays truthful during a staggered multi-machine release.
const platforms = {};
for (const os of ["windows", "macos", "linux"]) {
  const candidates = TARGETS.filter((t) => t.os === os);
  const preferred = launcherTargets[PREFERRED[os]];
  const available = preferred ?? candidates.map((t) => launcherTargets[t.key]).find(Boolean);
  platforms[os] = available ? {
    ...available,
    variants: candidates
      .map((target) => launcherTargets[target.key])
      .filter(Boolean),
  } : {
    available: false,
    label: candidates[0].label,
    ext: candidates[0].ext,
  };
}

// Client-Binaries: version + sha256 pro OS, damit der Launcher gezielt
// nachladen kann, wenn sich die veröffentlichte Version ändert.
const client = { version };
const clientTargets = {};
for (const target of TARGETS) {
  const found = files.find((f) => target.client.test(f) && statSync(join(dir, f)).isFile());
  if (found) {
    const full = join(dir, found);
    clientTargets[target.key] = {
      file: found,
      url: `/downloads/${found}`,
      size: statSync(full).size,
      sha256: sha256(full),
    };
  }
}
for (const os of ["windows", "macos", "linux"]) {
  const preferred = clientTargets[PREFERRED[os]];
  const fallback = TARGETS.filter((t) => t.os === os)
    .map((t) => clientTargets[t.key]).find(Boolean);
  if (preferred ?? fallback) client[os] = preferred ?? fallback;
}

// Versions-Archiv: downloads/client/<version>/<binary> — damit der Launcher
// auch ältere Client-Versionen anbieten kann. Neueste zuerst.
const clientVersions = [];
try {
  const versDir = join(dir, "client");
  const versions = readdirSync(versDir)
    .filter((v) => statSync(join(versDir, v)).isDirectory())
    .sort((a, b) =>
      b.localeCompare(a, undefined, { numeric: true, sensitivity: "base" }),
    );
  for (const v of versions) {
    const entry = { version: v, targets: {} };
    let any = false;
    for (const target of TARGETS) {
      const f = readdirSync(join(versDir, v)).find((f) => target.client.test(f));
      if (f) {
        const full = join(versDir, v, f);
        entry.targets[target.key] = {
          file: f,
          url: `/downloads/client/${v}/${f}`,
          size: statSync(full).size,
          sha256: sha256(full),
        };
        any = true;
      }
    }
    for (const os of ["windows", "macos", "linux"]) {
      const preferred = entry.targets[PREFERRED[os]];
      const fallback = TARGETS.filter((t) => t.os === os)
        .map((t) => entry.targets[t.key]).find(Boolean);
      if (preferred ?? fallback) entry[os] = preferred ?? fallback;
    }
    if (any) clientVersions.push(entry);
  }
} catch {
  // kein Archiv — Feld bleibt leer.
}

const manifest = {
  product: "DolphinClient",
  minecraft,
  version,
  generatedAt: new Date().toISOString(),
  platforms,
  launcherTargets,
  client,
  clientTargets,
  clientVersions,
};

writeFileSync(join(dir, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
const ready = Object.values(platforms).filter((p) => p.available).length;
const clientReady = Object.keys(clientTargets).length;
console.log(
  `manifest.json geschrieben (Launcher ${ready}/3 OS, Client ${clientReady}/5 Targets, ` +
    `Version ${manifest.version}, Minecraft ${minecraft}).`,
);
