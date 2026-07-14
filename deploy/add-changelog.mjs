#!/usr/bin/env node
// Fügt einen neuen Eintrag OBEN in website/app/changelog/data.ts ein und macht
// den bisher obersten Eintrag „nicht mehr aktuell" (entfernt dessen „Aktuell · "
// aus der date-Zeile). Wird von ../release.sh aufgerufen, kann aber auch von
// Hand benutzt werden.
//
// Aufruf:
//   node deploy/add-changelog.mjs <version> <headline> <itemsFile> [dataFile]
//     version    z. B. 0.21.0            (wird als "v0.21.0" gespeichert)
//     headline   kurze Titelzeile        (wird zu "Aktuell · <headline>")
//     itemsFile  Textdatei, ein Bullet pro Zeile (leere Zeilen werden ignoriert)
//     dataFile   optional, Standard: website/app/changelog/data.ts
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const [, , version, headline, itemsFile, dataFileArg] = process.argv;
if (!version || !headline || !itemsFile) {
  console.error(
    "Aufruf: add-changelog.mjs <version> <headline> <itemsFile> [dataFile]",
  );
  process.exit(1);
}

const dataFile =
  dataFileArg ||
  fileURLToPath(new URL("../website/app/changelog/data.ts", import.meta.url));

let src = readFileSync(dataFile, "utf8");

const items = readFileSync(itemsFile, "utf8")
  .split("\n")
  .map((s) => s.replace(/\r$/, "").trim())
  .filter(Boolean)
  .map((s) => s.replace(/^[-*•·]\s*/, "")); // führende Aufzählungszeichen entfernen

if (!items.length) {
  console.error("Keine Changelog-Punkte angegeben.");
  process.exit(1);
}

const v = version.startsWith("v") ? version : `v${version}`;

// Idempotent: Existiert der Eintrag schon, wird er bei FORCE=1 ersetzt, sonst
// unverändert übernommen (exit 0). So kann release.sh einen Schritt oder alles
// gefahrlos wiederholen, ohne dass der Changelog abbricht oder sich verdoppelt.
if (src.includes(`v: "${v}"`)) {
  if (process.env.FORCE !== "1") {
    console.log(`Changelog: ${v} existiert bereits — übernommen (keine Änderung).`);
    process.exit(0);
  }
  const vEsc = v.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const rm = new RegExp(`\\n {2}\\{\\n {4}v: "${vEsc}",[\\s\\S]*?\\n {2}\\},`);
  src = src.replace(rm, "");
}

const marker = "export const CHANGES: ChangeEntry[] = [";
const idx = src.indexOf(marker);
if (idx === -1) {
  console.error(`CHANGES-Array nicht gefunden in ${dataFile}`);
  process.exit(1);
}

const head = src.slice(0, idx + marker.length);
let tail = src.slice(idx + marker.length);

// Dem bisher obersten Eintrag das „Aktuell · " nehmen (nur erstes Vorkommen).
tail = tail.replace(/date:\s*"Aktuell\s*·\s*/, 'date: "');

const block =
  `\n  {\n` +
  `    v: ${JSON.stringify(v)},\n` +
  `    date: ${JSON.stringify(`Aktuell · ${headline}`)},\n` +
  `    items: [\n` +
  items.map((it) => `      ${JSON.stringify(it)},`).join("\n") +
  `\n    ],\n` +
  `  },`;

writeFileSync(dataFile, head + block + tail);
console.log(`Changelog: ${v} mit ${items.length} Punkt(en) oben eingefügt.`);
