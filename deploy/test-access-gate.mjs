#!/usr/bin/env node
// Selbsttest für das Origin-Gate (deploy/access-gate.mjs).
//
// Es gibt kein Cloudflare in diesem Test: wir erzeugen ein eigenes RSA-Paar,
// liefern den öffentlichen Schlüssel unter einer gefälschten "certs"-Adresse aus
// (ACCESS_CERTS_URL) und schicken dem Gate selbst signierte Token. Geprüft wird
// vor allem, was ABGELEHNT werden muss — ein Türsteher, der jeden reinlässt,
// fällt sonst nicht auf.
//
//   node deploy/test-access-gate.mjs
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { generateKeyPairSync, createSign, randomUUID } from "node:crypto";
import { fileURLToPath } from "node:url";

const GATE = fileURLToPath(new URL("./access-gate.mjs", import.meta.url));
const TEAM = "selftest.cloudflareaccess.com";
const AUD = "aud-tag-for-the-selftest";
const CERTS_PORT = 8799;
const GATE_PORT = 8798;

const { publicKey, privateKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
const KID = "test-key-1";
const jwk = { ...publicKey.export({ format: "jwk" }), kid: KID, alg: "RS256", use: "sig" };
// Ein zweites Paar, mit dem "falsch signiert" wirklich falsch ist.
const other = generateKeyPairSync("rsa", { modulusLength: 2048 });

const certs = createServer((_req, res) => {
  res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ keys: [jwk] }));
});
await new Promise((r) => certs.listen(CERTS_PORT, "127.0.0.1", r));

const b64 = (obj) => Buffer.from(JSON.stringify(obj)).toString("base64url");
function token({ head = {}, claims = {}, key = privateKey } = {}) {
  const now = Math.floor(Date.now() / 1000);
  const h = { alg: "RS256", kid: KID, typ: "JWT", ...head };
  const c = {
    iss: `https://${TEAM}`,
    aud: [AUD],
    exp: now + 600,
    iat: now,
    email: "user@example.invalid",
    sub: randomUUID(),
    ...claims,
  };
  const signed = `${b64(h)}.${b64(c)}`;
  const sig = createSign("RSA-SHA256").update(signed).sign(key).toString("base64url");
  return `${signed}.${sig}`;
}

const gate = spawn(process.execPath, [GATE], {
  env: {
    ...process.env,
    ACCESS_TEAM_DOMAIN: TEAM,
    ACCESS_AUD: AUD,
    ACCESS_ALLOWED_EMAILS: "user@example.invalid",
    ACCESS_CERTS_URL: `http://127.0.0.1:${CERTS_PORT}`,
    ACCESS_LISTEN: `127.0.0.1:${GATE_PORT}`,
  },
  stdio: ["ignore", "ignore", "inherit"],
});
process.on("exit", () => gate.kill());

// Auf den Start warten.
for (let i = 0; i < 50; i++) {
  try {
    await fetch(`http://127.0.0.1:${GATE_PORT}/health`);
    break;
  } catch {
    await new Promise((r) => setTimeout(r, 100));
  }
}

async function ask(path, headers) {
  const res = await fetch(`http://127.0.0.1:${GATE_PORT}${path}`, { headers });
  return { status: res.status, reason: res.headers.get("x-access-reason"), body: await res.text() };
}

const bearer = (t) => ({ "cf-access-jwt-assertion": t });
let failed = 0;
async function expect(name, path, headers, want) {
  const r = await ask(path, headers);
  const ok = r.status === want;
  if (!ok) failed++;
  console.log(
    `${ok ? "  ok  " : "FAIL  "}${name}: ${r.status}${r.reason ? ` (${r.reason})` : ""}` +
      (ok ? "" : ` — erwartet ${want}`),
  );
  return r;
}

console.log("Origin-Gate: was durchgelassen werden muss");
await expect("gültiges Token", "/verify", bearer(token()), 200);
await expect(
  "gültiges Token im Cookie",
  "/verify",
  { cookie: `CF_Authorization=${token()}; other=1` },
  200,
);
const who = await expect("whoami nennt die Identität", "/whoami", bearer(token()), 200);
if (!who.body.includes("user@example.invalid")) {
  console.log("FAIL  whoami ohne E-Mail:", who.body);
  failed++;
} else {
  console.log("  ok  whoami-Antwort:", who.body.trim());
}

console.log("\nOrigin-Gate: was abgewiesen werden muss");
await expect("gar kein Token", "/verify", {}, 401);
await expect("Unsinn statt Token", "/verify", bearer("nicht.ein.jwt"), 401);
await expect("fremd signiert", "/verify", bearer(token({ key: other.privateKey })), 401);
await expect("unbekannter Schlüssel", "/verify", bearer(token({ head: { kid: "anderer" } })), 401);
await expect("alg=none", "/verify", bearer(token({ head: { alg: "none" } })), 401);
await expect(
  "abgelaufen",
  "/verify",
  bearer(token({ claims: { exp: Math.floor(Date.now() / 1000) - 120 } })),
  401,
);
await expect("falsche AUD", "/verify", bearer(token({ claims: { aud: ["andere-app"] } })), 401);
await expect(
  "falscher Aussteller",
  "/verify",
  bearer(token({ claims: { iss: "https://boese.cloudflareaccess.com" } })),
  401,
);
await expect(
  "nicht freigegebene E-Mail",
  "/verify",
  bearer(token({ claims: { email: "user@example.invalid" } })),
  401,
);
await expect(
  "manipulierte Nutzdaten",
  "/verify",
  bearer((() => {
    const t = token().split(".");
    const c = JSON.parse(Buffer.from(t[1], "base64url").toString());
    c.email = "user@example.invalid";
    return `${t[0]}.${Buffer.from(JSON.stringify(c)).toString("base64url")}.${t[2]}`;
  })()),
  401,
);

// Ohne Konfiguration darf NICHTS durchgehen — auch kein gültiges Token.
console.log("\nOrigin-Gate: ohne Konfiguration bleibt alles zu");
const closed = spawn(process.execPath, [GATE], {
  env: { ...process.env, ACCESS_TEAM_DOMAIN: "", ACCESS_AUD: "", ACCESS_LISTEN: "127.0.0.1:8797" },
  stdio: ["ignore", "ignore", "inherit"],
});
process.on("exit", () => closed.kill());
for (let i = 0; i < 50; i++) {
  try {
    await fetch("http://127.0.0.1:8797/health");
    break;
  } catch {
    await new Promise((r) => setTimeout(r, 100));
  }
}
const res = await fetch("http://127.0.0.1:8797/verify", { headers: bearer(token()) });
const okClosed = res.status === 503;
if (!okClosed) failed++;
console.log(`${okClosed ? "  ok  " : "FAIL  "}unkonfiguriert: ${res.status} (erwartet 503)`);

gate.kill();
closed.kill();
certs.close();
console.log(failed ? `\n${failed} Test(s) FEHLGESCHLAGEN` : "\nAlle Tests bestanden.");
process.exit(failed ? 1 : 0);
