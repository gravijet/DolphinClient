#!/usr/bin/env node
// Pull the LIVE changelog back into the repository.
//
// Two things write the version history: `release.sh` (via add-changelog.mjs,
// into the git-tracked website/app/changelog/changelog.json) and the admin
// portal (via admin-api.mjs, straight into the published
// downloads/changelog.json). Without this script the second kind of edit would
// be silently reverted by the next release, because the release republishes
// the repo copy.
//
// So: before a release adds its entry, the live file wins. Anything edited in
// the portal since the last release is copied back into the repo, and the two
// stay one history.
//
//   node deploy/sync-changelog.mjs [--quiet]
// Exit code is 0 whether or not anything changed; a missing live file is fine
// (fresh machine), an unreadable one is reported and left alone.
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const REPO = fileURLToPath(new URL("../website/app/changelog/changelog.json", import.meta.url));
const LIVE =
  process.env.DOLPHIN_CHANGELOG ||
  `${process.env.DOLPHIN_DOWNLOADS || "/var/www/dolphinclient.de/downloads"}/changelog.json`;
const quiet = process.argv.includes("--quiet");
const say = (m) => !quiet && console.log(m);

if (!existsSync(LIVE)) {
  say(`sync-changelog: no live file at ${LIVE} — nothing to pull in.`);
  process.exit(0);
}

let live;
try {
  live = JSON.parse(readFileSync(LIVE, "utf8"));
} catch (e) {
  console.error(`sync-changelog: ${LIVE} is not readable JSON (${e.message}) — leaving the repo copy alone.`);
  process.exit(0);
}
if (!Array.isArray(live) || !live.length) {
  console.error("sync-changelog: the live file is empty — leaving the repo copy alone.");
  process.exit(0);
}

const before = existsSync(REPO) ? readFileSync(REPO, "utf8") : "";
const after = `${JSON.stringify(live, null, 2)}\n`;
if (before.trim() === after.trim()) {
  say("sync-changelog: repository already matches the live changelog.");
  process.exit(0);
}

// What actually differs, so the release log says something useful.
let old = [];
try {
  old = JSON.parse(before);
} catch {
  /* first run or a broken file: just take the live one */
}
const oldByV = new Map(old.map((e) => [e.v, JSON.stringify(e)]));
const liveByV = new Map(live.map((e) => [e.v, JSON.stringify(e)]));
const added = [...liveByV.keys()].filter((v) => !oldByV.has(v));
const removed = [...oldByV.keys()].filter((v) => !liveByV.has(v));
const changed = [...liveByV.keys()].filter((v) => oldByV.has(v) && oldByV.get(v) !== liveByV.get(v));

writeFileSync(REPO, after);
say(
  `sync-changelog: pulled ${live.length} entries from ${LIVE}` +
    `${added.length ? ` — added ${added.join(", ")}` : ""}` +
    `${changed.length ? ` — edited ${changed.join(", ")}` : ""}` +
    `${removed.length ? ` — removed ${removed.join(", ")}` : ""}`,
);
