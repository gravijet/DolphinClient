#!/usr/bin/env node
// Self-test for the changelog write API (deploy/admin-api.mjs).
//
// It runs against a throwaway web root in /tmp and a fake Cloudflare "certs"
// endpoint, so nothing here touches the live site. Two things are checked: that
// editing works end to end, and — more importantly — that everything which must
// NOT be possible is refused: writing without a token, versions or file names
// that escape their folder, files that only claim to be images, an empty
// history, duplicate versions.
//
//   node deploy/test-admin-api.mjs
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { generateKeyPairSync, createSign, randomUUID } from "node:crypto";
import { mkdtempSync, readFileSync, existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const API = fileURLToPath(new URL("./admin-api.mjs", import.meta.url));
const TEAM = "selftest.cloudflareaccess.com";
const AUD = "aud-tag-for-the-selftest";
const CERTS_PORT = 8795;
const API_PORT = 8794;

const root = mkdtempSync(join(tmpdir(), "dolphin-admin-test-"));
const WEBROOT = join(root, "www");
const HISTORY = join(root, "history");
mkdirSync(join(WEBROOT, "downloads"), { recursive: true });

const { publicKey, privateKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
const KID = "test-key-1";
const jwk = { ...publicKey.export({ format: "jwk" }), kid: KID, alg: "RS256", use: "sig" };

const certs = createServer((_req, res) => {
  res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ keys: [jwk] }));
});
await new Promise((r) => certs.listen(CERTS_PORT, "127.0.0.1", r));

const b64 = (obj) => Buffer.from(JSON.stringify(obj)).toString("base64url");
function token(claims = {}) {
  const now = Math.floor(Date.now() / 1000);
  const h = { alg: "RS256", kid: KID, typ: "JWT" };
  const c = {
    iss: `https://${TEAM}`,
    aud: [AUD],
    exp: now + 600,
    iat: now,
    email: "owner@example.com",
    sub: randomUUID(),
    ...claims,
  };
  const signed = `${b64(h)}.${b64(c)}`;
  return `${signed}.${createSign("RSA-SHA256").update(signed).sign(privateKey).toString("base64url")}`;
}

const api = spawn(process.execPath, [API], {
  env: {
    ...process.env,
    ACCESS_TEAM_DOMAIN: TEAM,
    ACCESS_AUD: AUD,
    ACCESS_CERTS_URL: `http://127.0.0.1:${CERTS_PORT}`,
    ADMIN_API_LISTEN: `127.0.0.1:${API_PORT}`,
    DOLPHIN_WEBROOT: WEBROOT,
    DOLPHIN_HISTORY: HISTORY,
  },
  stdio: ["ignore", "ignore", "inherit"],
});
process.on("exit", () => api.kill());

for (let i = 0; i < 50; i++) {
  try {
    await fetch(`http://127.0.0.1:${API_PORT}/health`);
    break;
  } catch {
    await new Promise((r) => setTimeout(r, 100));
  }
}

let failed = 0;
async function call(method, path, { body, auth = true, raw } = {}) {
  const headers = {};
  if (auth) headers["cf-access-jwt-assertion"] = token();
  if (body !== undefined) headers["content-type"] = "application/json";
  const res = await fetch(`http://127.0.0.1:${API_PORT}${path}`, {
    method,
    headers,
    body: raw ?? (body === undefined ? undefined : JSON.stringify(body)),
  });
  const text = await res.text();
  let parsed = null;
  try {
    parsed = JSON.parse(text);
  } catch {
    /* not json */
  }
  return { status: res.status, body: parsed, text };
}

function check(name, ok, detail = "") {
  if (!ok) failed++;
  console.log(`${ok ? "  ok  " : "FAIL  "}${name}${detail ? `: ${detail}` : ""}`);
}

async function expect(name, want, method, path, opts) {
  const r = await call(method, path, opts);
  check(name, r.status === want, `${r.status}${r.status === want ? "" : ` (wanted ${want})`}${r.body?.error ? ` — ${r.body.error}` : ""}`);
  return r;
}

const entry = (v, extra = {}) => ({
  v,
  date: `Something happened in ${v}`,
  items: ["A first line.", "A second line."],
  ...extra,
});

const live = () => JSON.parse(readFileSync(join(WEBROOT, "downloads", "changelog.json"), "utf8"));

console.log("Admin API: what must work");
await expect("publish the first entry", 201, "POST", "/changelog", { body: entry("0.1.0") });
await expect("publish a second entry", 201, "POST", "/changelog", { body: entry("0.2.0") });
check("newest first", live()[0].v === "v0.2.0", live().map((e) => e.v).join(", "));
check('only the newest is "Current"', live()[0].date.startsWith("Current · ") && !live()[1].date.startsWith("Current · "));
check("a version is normalised to v-form", live()[1].v === "v0.1.0");

const edited = await expect("edit an entry", 200, "PUT", "/changelog/0.1.0", {
  body: { ...entry("0.1.0"), items: ["Rewritten."] },
});
check("the edit is stored", edited.body.entries.find((e) => e.v === "v0.1.0").items[0] === "Rewritten.");

await expect("reorder the whole history", 200, "PUT", "/changelog", {
  body: { entries: [entry("0.1.0"), entry("0.2.0")] },
});
check("the new order is stored", live()[0].v === "v0.1.0");

// A real 1×1 PNG.
const png = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64",
);
await expect("upload a screenshot", 201, "POST", "/shots/0.2.0/shot-one.png", { raw: png });
check("the file is on disk", existsSync(join(WEBROOT, "downloads", "shots", "v0.2.0", "shot-one.png")));
const listed = await expect("list the screenshots", 200, "GET", "/shots/0.2.0");
check("the listing shows it", listed.body.files[0]?.src === "shot-one.png");

await expect("attach the shot to its entry", 200, "PUT", "/changelog/0.2.0", {
  body: entry("0.2.0", { shots: [{ src: "shot-one.png", alt: "A picture" }] }),
});
const removed = await expect("delete the screenshot", 200, "DELETE", "/shots/0.2.0/shot-one.png");
check(
  "deleting a shot also unlinks it from the entry",
  !removed.body.entries.find((e) => e.v === "v0.2.0").shots,
);
check("the file is gone", !existsSync(join(WEBROOT, "downloads", "shots", "v0.2.0", "shot-one.png")));

const backups = await expect("list the backups", 200, "GET", "/history");
check("every write was backed up", backups.body.backups.length >= 5, `${backups.body.backups.length}`);
// The newest backup is the state just before the last write — restoring it
// must bring the screenshot we just deleted back.
const newest = backups.body.backups[0].file;
await expect("restore a backup", 200, "POST", `/restore/${newest}`);
check(
  "the restore undid the last change",
  live().find((e) => e.v === "v0.2.0")?.shots?.[0]?.src === "shot-one.png",
);

await expect("delete an entry", 200, "DELETE", "/changelog/0.2.0");
check("it is gone", !live().some((e) => e.v === "v0.2.0"));

console.log("\nAdmin API: what must be refused");
await expect("no token at all", 401, "GET", "/changelog", { auth: false });
await expect("writing without a token", 401, "POST", "/changelog", {
  auth: false,
  body: entry("9.9.9"),
});
await expect("a version that is not a version", 400, "POST", "/changelog", {
  body: entry("../../etc/passwd"),
});
await expect("a version with a path in it", 400, "POST", "/changelog", { body: entry("0.1.0/../x") });
await expect("an entry with no bullets", 400, "POST", "/changelog", {
  body: { v: "9.9.9", date: "x", items: [] },
});
await expect("an entry with no headline", 400, "POST", "/changelog", {
  body: { v: "9.9.9", date: "   ", items: ["x"] },
});
await expect("a duplicate version", 409, "POST", "/changelog", { body: entry("0.1.0") });
await expect("editing an entry that does not exist", 404, "PUT", "/changelog/7.7.7", {
  body: entry("7.7.7"),
});
await expect("a screenshot name that escapes its folder", 400, "POST", "/shots/0.1.0/..%2F..%2Fx.png", {
  raw: png,
});
await expect("a screenshot with a slash in the name", 404, "POST", "/shots/0.1.0/sub/dir.png", {
  raw: png,
});
await expect("a screenshot that is not an image", 400, "POST", "/shots/0.1.0/evil.png", {
  raw: Buffer.from("<?php system($_GET['c']); ?>"),
});
await expect("an executable renamed to .png", 400, "POST", "/shots/0.1.0/x.png", {
  raw: Buffer.from([0x7f, 0x45, 0x4c, 0x46, 1, 1, 1, 0, 0, 0, 0, 0]),
});
await expect("a shot file with no image suffix", 400, "POST", "/shots/0.1.0/script.sh", { raw: png });
await expect("emptying the history", 400, "DELETE", "/changelog/0.1.0");
await expect("restoring something that is not a backup", 400, "POST", "/restore/..%2F..%2Fpasswd");
await expect("an unknown route", 404, "GET", "/nope");

check("the live file is still valid JSON", Array.isArray(live()) && live().length === 1);

// Unconfigured: nothing may pass, not even a valid token.
console.log("\nAdmin API: with no configuration everything stays shut");
const closed = spawn(process.execPath, [API], {
  env: {
    ...process.env,
    ACCESS_TEAM_DOMAIN: "",
    ACCESS_AUD: "",
    ADMIN_API_LISTEN: "127.0.0.1:8793",
    DOLPHIN_WEBROOT: WEBROOT,
    DOLPHIN_HISTORY: HISTORY,
  },
  stdio: ["ignore", "ignore", "inherit"],
});
process.on("exit", () => closed.kill());
for (let i = 0; i < 50; i++) {
  try {
    await fetch("http://127.0.0.1:8793/health");
    break;
  } catch {
    await new Promise((r) => setTimeout(r, 100));
  }
}
const res = await fetch("http://127.0.0.1:8793/changelog", {
  method: "DELETE",
  headers: { "cf-access-jwt-assertion": token() },
});
check(`unconfigured: ${res.status}`, res.status === 503);

console.log(failed ? `\n${failed} test(s) FAILED` : "\nAll tests passed.");
certs.close();
api.kill();
closed.kill();
process.exit(failed ? 1 : 0);
