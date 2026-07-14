#!/usr/bin/env node
// Schlägt AUTOMATISCH einen Changelog aus den aktuell geänderten Dateien vor —
// damit man beim Release nichts von Hand schreiben muss. Gibt die Überschrift
// in Zeile 1 aus, danach je eine Zeile pro Stichpunkt. Wird von ../release.sh
// aufgerufen (Standard-Vorschlag, den man mit Enter übernimmt).
import { execSync } from "node:child_process";

let out = "";
try {
  out = execSync("git status --porcelain", { encoding: "utf8" });
} catch {
  out = "";
}

const paths = out
  .split("\n")
  .filter(Boolean)
  .map((l) => {
    let p = l.slice(3); // 2 Status-Zeichen + Leerzeichen
    if (p.includes(" -> ")) p = p.split(" -> ").pop(); // Umbenennungen
    return p.replace(/^"|"$/g, "").trim();
  })
  .filter(Boolean);

// Reihenfolge zählt: die erste passende Regel gewinnt.
const RULES = [
  [/^client-rust\/src\/render|^client-rust\/src\/app\/offscreen|^client-rust\/src\/app\/entity|entity_models/, "Client", "Rendering & Grafik"],
  [/^client-rust\/src\/audio|blocksound/, "Client", "Audio & Sound"],
  [/^client-rust\/src\/app\/hud/, "Client", "HUD & Anzeige"],
  [/^client-rust\/src\/app\/mcui/, "Client", "Menüs & UI"],
  [/^client-rust\/src\/bridge/, "Client", "Server-Verbindung"],
  [/^client-rust\/src\/render\/entity_models|mob/, "Client", "Mobs & Modelle"],
  [/^client-rust/, "Client", "Kern & Sonstiges"],
  [/^launcher-native/, "Launcher", ""],
  [/^website/, "Website", ""],
  [/^deploy|\.sh$|^ANLEITUNG|README|\.md$|^package(-lock)?\.json$/, "Build/Release", ""],
];

const areas = new Map(); // Komponente -> Set von Bereichen
for (const p of paths) {
  for (const [re, comp, area] of RULES) {
    if (re.test(p)) {
      if (!areas.has(comp)) areas.set(comp, new Set());
      if (area) areas.get(comp).add(area);
      break;
    }
  }
}

const bullets = [];
const has = (c) => areas.has(c);
if (has("Client")) {
  let a = [...areas.get("Client")];
  // „Kern & Sonstiges" ans Ende, dann auf höchstens 4 Bereiche kürzen.
  a.sort((x, y) => (x === "Kern & Sonstiges" ? 1 : 0) - (y === "Kern & Sonstiges" ? 1 : 0));
  let tail = "";
  if (a.length > 4) { a = a.slice(0, 4); tail = " u. a."; }
  bullets.push(
    a.length
      ? `Client verbessert: ${a.join(", ")}${tail}`
      : "Client verbessert und Fehler behoben",
  );
}
if (has("Launcher"))
  bullets.push("Launcher überarbeitet — stabiler und aufgeräumter");
if (has("Website")) bullets.push("Website aktualisiert und verfeinert");
if (has("Build/Release"))
  bullets.push("Build- und Veröffentlichungs-Ablauf verbessert");
if (!bullets.length)
  bullets.push("Kleinere Verbesserungen und Fehlerbehebungen");

// Überschrift aus den sichtbaren Komponenten (ohne „Build/Release").
const label = [...areas.keys()].filter((c) => c !== "Build/Release");
let headline;
if (label.length === 0) headline = "Wartung & Feinschliff";
else if (label.length === 1) headline = `${label[0]}-Update & Verbesserungen`;
else
  headline = `${label.slice(0, -1).join(", ")} & ${label.slice(-1)} verbessert`;

console.log(headline);
for (const b of bullets) console.log(b);
