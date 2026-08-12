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
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// Positional args, plus an optional `--shots <fileOrDir>` anywhere in the line.
const argv = process.argv.slice(2);
let shotsArg = "";
const shotsAt = argv.indexOf("--shots");
if (shotsAt !== -1) {
  shotsArg = argv[shotsAt + 1] || "";
  argv.splice(shotsAt, 2);
}
const [version, headline, itemsFile, dataFileArg] = argv;
if (!version || !headline || !itemsFile) {
  console.error(
    "Aufruf: add-changelog.mjs <version> <headline> <itemsFile> [dataFile] [--shots <datei|ordner>]",
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

// Screenshots for this release. `--shots` takes either
//   * a text file with one `dateiname | Bildunterschrift` per line, or
//   * a directory of PNGs (the file name becomes the caption).
// The pictures themselves live in `screenshots/<v>/` and are published to
// `downloads/shots/<v>/` at release time — the website reads them at runtime.
/** @type {{src:string,alt:string}[]} */
let shots = [];
if (shotsArg) {
  if (!existsSync(shotsArg)) {
    console.error(`Screenshots: ${shotsArg} existiert nicht — Eintrag ohne Bilder.`);
  } else if (statSync(shotsArg).isDirectory()) {
    shots = readdirSync(shotsArg)
      .filter((f) => /\.(png|jpg|jpeg|webp)$/i.test(f))
      .sort()
      .map((f) => ({
        src: f,
        alt: basename(f, f.slice(f.lastIndexOf("."))).replace(/[_-]+/g, " "),
      }));
  } else {
    shots = readFileSync(shotsArg, "utf8")
      .split("\n")
      .map((s) => s.replace(/\r$/, "").trim())
      .filter((s) => s && !s.startsWith("#"))
      .map((line) => {
        const [src, ...rest] = line.split("|");
        return { src: src.trim(), alt: rest.join("|").trim() || src.trim() };
      })
      .filter((s) => s.src);
  }
  // Eine Bildunterschrift ohne Bild wäre ein kaputtes Bild auf der Website —
  // also raus damit. Die Bilder liegen neben der Liste bzw. IM Ordner.
  const dir = statSync(shotsArg).isDirectory() ? shotsArg : dirname(shotsArg);
  const missing = shots.filter((s) => !existsSync(join(dir, s.src)));
  if (missing.length) {
    console.error(
      `Screenshots: ${missing.length} Datei(en) fehlen und werden ausgelassen: ` +
        missing.map((s) => s.src).join(", "),
    );
  }
  shots = shots.filter((s) => existsSync(join(dir, s.src)));
  console.log(`Screenshots: ${shots.length} Bild(er) für ${v}.`);
}

/** @type {{v:string,date:string,items:string[],shots?:{src:string,alt:string}[]}[]} */
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
  // Den Text nicht anfassen — aber Screenshots nachtragen, falls der Eintrag
  // noch keine hat (ein wiederholter Release-Schritt soll Bilder ergänzen).
  if (shots.length && !changes[existingIdx].shots?.length) {
    changes[existingIdx].shots = shots;
    writeFileSync(dataFile, JSON.stringify(changes, null, 2) + "\n");
    console.log(`Changelog: ${v} existiert bereits — ${shots.length} Screenshot(s) ergänzt.`);
  } else {
    console.log(`Changelog: ${v} existiert bereits — übernommen (keine Änderung).`);
  }
  process.exit(0);
}
if (existingIdx !== -1) changes.splice(existingIdx, 1);

// Dem bisher obersten Eintrag das "Current · " nehmen (die Website ist
// durchgehend englisch; ältere Einträge tragen noch das alte "Aktuell · ").
if (changes.length && typeof changes[0].date === "string") {
  changes[0].date = changes[0].date.replace(/^(Current|Aktuell)\s*·\s*/, "");
}

changes.unshift(
  shots.length
    ? { v, date: `Current · ${headline}`, items, shots }
    : { v, date: `Current · ${headline}`, items },
);

writeFileSync(dataFile, JSON.stringify(changes, null, 2) + "\n");
console.log(
  `Changelog: ${v} mit ${items.length} Punkt(en)` +
    (shots.length ? ` und ${shots.length} Screenshot(s)` : "") +
    " oben eingefügt.",
);
