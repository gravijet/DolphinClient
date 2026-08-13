#!/usr/bin/env node
// Self-test for the origin gate (deploy/access-gate.mjs).
//
// There is no Cloudflare in this test: we generate our own RSA pair, serve the
// public key from a fake "certs" address (ACCESS_CERTS_URL) and send the gate
// tokens we signed ourselves. What matters most is what must be REJECTED — a
// doorman who lets everyone in looks fine otherwise.
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
// A second pair, so that "wrongly signed" really is wrong.
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
    email: "owner@example.com",
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
    ACCESS_ALLOWED_EMAILS: "owner@example.com",
    ACCESS_CERTS_URL: `http://127.0.0.1:${CERTS_PORT}`,
    ACCESS_LISTEN: `127.0.0.1:${GATE_PORT}`,
  },
  stdio: ["ignore", "ignore", "inherit"],
});
process.on("exit", () => gate.kill());

// Wait for it to come up.
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
      (ok ? "" : ` — expected ${want}`),
  );
  return r;
}

console.log("Origin gate: what must be let through");
await expect("valid token", "/verify", bearer(token()), 200);
await expect(
  "valid token in the cookie",
  "/verify",
  { cookie: `CF_Authorization=${token()}; other=1` },
  200,
);
const who = await expect("whoami names the identity", "/whoami", bearer(token()), 200);
if (!who.body.includes("owner@example.com")) {
  console.log("FAIL  whoami without an e-mail:", who.body);
  failed++;
} else {
  console.log("  ok  whoami answer:", who.body.trim());
}

console.log("\nOrigin gate: what must be rejected");
await expect("no token at all", "/verify", {}, 401);
await expect("nonsense instead of a token", "/verify", bearer("not.a.jwt"), 401);
await expect("foreign signature", "/verify", bearer(token({ key: other.privateKey })), 401);
await expect("unknown key", "/verify", bearer(token({ head: { kid: "someone-else" } })), 401);
await expect("alg=none", "/verify", bearer(token({ head: { alg: "none" } })), 401);
await expect(
  "expired",
  "/verify",
  bearer(token({ claims: { exp: Math.floor(Date.now() / 1000) - 120 } })),
  401,
);
await expect("wrong AUD", "/verify", bearer(token({ claims: { aud: ["another-app"] } })), 401);
await expect(
  "wrong issuer",
  "/verify",
  bearer(token({ claims: { iss: "https://evil.cloudflareaccess.com" } })),
  401,
);
await expect(
  "e-mail not on the allow-list",
  "/verify",
  bearer(token({ claims: { email: "stranger@example.com" } })),
  401,
);
await expect(
  "tampered payload",
  "/verify",
  bearer((() => {
    const t = token().split(".");
    const c = JSON.parse(Buffer.from(t[1], "base64url").toString());
    c.email = "stranger@example.com";
    return `${t[0]}.${Buffer.from(JSON.stringify(c)).toString("base64url")}.${t[2]}`;
  })()),
  401,
);

// With no configuration NOTHING may pass — not even a valid token.
console.log("\nOrigin gate: with no configuration everything stays shut");
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
console.log(`${okClosed ? "  ok  " : "FAIL  "}unconfigured: ${res.status} (expected 503)`);

gate.kill();
closed.kill();
certs.close();
console.log(failed ? `\n${failed} test(s) FAILED` : "\nAll tests passed.");
process.exit(failed ? 1 : 0);
