#!/usr/bin/env node
// Fügt einen neuen Eintrag OBEN in website/app/changelog/changelog.json ein und
// macht den bisher obersten Eintrag „nicht mehr aktuell" (entfernt dessen
// "Current · " aus der date-Zeile). Wird von ../release.sh aufgerufen, kann aber
// auch von Hand benutzt werden.
//
// changelog.json ist die EINZIGE Quelle der Wahrheit für die Versionshistorie.
// Sie wird bei jedem Release nach downloads/changelog.json veröffentlicht und
// von der Website ZUR LAUFZEIT geladen — deshalb muss die statische Website
// NICHT mehr neu gebaut werden, nur weil ein Changelog-Eintrag dazukommt.
//
// Aufruf:
//   node deploy/add-changelog.mjs <version> <headline> <itemsFile> [dataFile]
//     version    z. B. 0.21.0            (wird als "v0.21.0" gespeichert)
//     headline   kurze Titelzeile        (wird zu "Current · <headline>")
//     itemsFile  Textdatei, ein Bullet pro Zeile (leere Zeilen werden ignoriert)
//     dataFile   optional, Standard: website/app/changelog/changelog.json
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
  fileURLToPath(
    new URL("../website/app/changelog/changelog.json", import.meta.url),
  );

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

/** @type {{v:string,date:string,items:string[]}[]} */
let changes = [];
try {
  changes = JSON.parse(readFileSync(dataFile, "utf8"));
  if (!Array.isArray(changes)) changes = [];
} catch {
  changes = [];
}

// Idempotent: Existiert der Eintrag schon, wird er bei FORCE=1 ersetzt, sonst
// unverändert übernommen (exit 0). So kann release.sh einen Schritt oder alles
// gefahrlos wiederholen, ohne dass der Changelog abbricht oder sich verdoppelt.
const existingIdx = changes.findIndex((c) => c.v === v);
if (existingIdx !== -1 && process.env.FORCE !== "1") {
  console.log(`Changelog: ${v} existiert bereits — übernommen (keine Änderung).`);
  process.exit(0);
}
if (existingIdx !== -1) changes.splice(existingIdx, 1);

// Dem bisher obersten Eintrag das "Current · " nehmen (die Website ist
// durchgehend englisch; ältere Einträge tragen noch das alte "Aktuell · ").
if (changes.length && typeof changes[0].date === "string") {
  changes[0].date = changes[0].date.replace(/^(Current|Aktuell)\s*·\s*/, "");
}

changes.unshift({ v, date: `Current · ${headline}`, items });

writeFileSync(dataFile, JSON.stringify(changes, null, 2) + "\n");
console.log(`Changelog: ${v} mit ${items.length} Punkt(en) oben eingefügt.`);
