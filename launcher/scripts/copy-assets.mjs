// Kopiert die Renderer-Assets (HTML/CSS) nach dist/renderer, damit der
// kompilierte Hauptprozess sie per loadFile findet. tsc kopiert nur .ts/.js.
import { cpSync, mkdirSync } from "node:fs";

mkdirSync("dist/renderer", { recursive: true });
cpSync("src/renderer/index.html", "dist/renderer/index.html");

console.log("Renderer-Assets nach dist/renderer kopiert.");
