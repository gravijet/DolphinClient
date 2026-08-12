#!/usr/bin/env node
// Origin-Gate für das Admin-Portal ("Zero Trust" heißt: auch der Ursprung
// vertraut niemandem). Cloudflare Access authentifiziert am Rand und hängt ein
// signiertes JWT an (`Cf-Access-Jwt-Assertion` bzw. Cookie `CF_Authorization`).
// Dieser winzige Dienst prüft dieses Token — Signatur, Aussteller, Zielgruppe,
// Laufzeit und optional die E-Mail-Adresse — und antwortet nginx per
// `auth_request` mit 200 oder 401.
//
// Ohne gültiges Token kommt NIEMAND an /admin: wer die Cloudflare-Adresse
// umgeht und direkt den Origin anspricht, hat kein Token und fliegt raus.
// Fehlt die Konfiguration, antwortet der Dienst 503 — im Zweifel ZU, nie offen.
//
// Konfiguration (/etc/dolphinclient/access.env):
//   ACCESS_TEAM_DOMAIN=deinteam.cloudflareaccess.com
//   ACCESS_AUD=<Application Audience (AUD) Tag der Access-Anwendung>
//   ACCESS_ALLOWED_EMAILS=user@example.invalid,user@example.invalid   (optional)
//   ACCESS_LISTEN=127.0.0.1:8787                              (optional)
//
// Start: systemd-Unit `dolphinclient-access.service` (siehe setup-zero-trust.sh).
import { createServer } from "node:http";
import { createPublicKey, createVerify } from "node:crypto";

const TEAM = (process.env.ACCESS_TEAM_DOMAIN || "").trim().replace(/^https?:\/\//, "").replace(/\/$/, "");
const AUD = (process.env.ACCESS_AUD || "").trim();
const ALLOWED = (process.env.ACCESS_ALLOWED_EMAILS || "")
  .split(",")
  .map((s) => s.trim().toLowerCase())
  .filter(Boolean);
const [HOST, PORT] = (process.env.ACCESS_LISTEN || "127.0.0.1:8787").split(":");
// ACCESS_CERTS_URL überschreibt die Schlüsselquelle — nur für den Selbsttest
// (deploy/test-access-gate.mjs). Im Betrieb bleibt es leer.
const CERTS_URL =
  process.env.ACCESS_CERTS_URL || (TEAM ? `https://${TEAM}/cdn-cgi/access/certs` : null);
const ISSUER = TEAM ? `https://${TEAM}` : null;

// --- Schlüssel-Cache -------------------------------------------------------
let keys = new Map(); // kid -> KeyObject
let keysFetched = 0;
let inflight = null;

async function refreshKeys(force = false) {
  if (!CERTS_URL) return;
  const age = Date.now() - keysFetched;
  if (!force && keys.size && age < 3_600_000) return;
  if (inflight) return inflight;
  inflight = (async () => {
    const res = await fetch(CERTS_URL, { signal: AbortSignal.timeout(8000) });
    if (!res.ok) throw new Error(`certs ${res.status}`);
    const body = await res.json();
    const next = new Map();
    for (const jwk of body.keys || []) {
      if (jwk.kty !== "RSA" || !jwk.kid) continue;
      try {
        next.set(jwk.kid, createPublicKey({ key: jwk, format: "jwk" }));
      } catch {
        /* skip unusable key */
      }
    }
    if (!next.size) throw new Error("no usable keys");
    keys = next;
    keysFetched = Date.now();
  })().finally(() => {
    inflight = null;
  });
  return inflight;
}

const b64url = (s) => Buffer.from(s.replace(/-/g, "+").replace(/_/g, "/"), "base64");

/** Verify an Access JWT. Returns {ok:true,email} or {ok:false,reason}. */
async function verify(token) {
  const parts = token.split(".");
  if (parts.length !== 3) return { ok: false, reason: "malformed" };
  let head;
  let claims;
  try {
    head = JSON.parse(b64url(parts[0]).toString("utf8"));
    claims = JSON.parse(b64url(parts[1]).toString("utf8"));
  } catch {
    return { ok: false, reason: "malformed" };
  }
  if (head.alg !== "RS256") return { ok: false, reason: "alg" };

  await refreshKeys();
  let key = keys.get(head.kid);
  if (!key) {
    // Cloudflare rotates its signing keys; an unknown kid is worth one refetch.
    await refreshKeys(true);
    key = keys.get(head.kid);
  }
  if (!key) return { ok: false, reason: "unknown-key" };

  const signed = `${parts[0]}.${parts[1]}`;
  const okSig = createVerify("RSA-SHA256").update(signed).verify(key, b64url(parts[2]));
  if (!okSig) return { ok: false, reason: "signature" };

  const now = Math.floor(Date.now() / 1000);
  if (typeof claims.exp === "number" && claims.exp < now - 30) return { ok: false, reason: "expired" };
  if (typeof claims.nbf === "number" && claims.nbf > now + 30) return { ok: false, reason: "not-yet-valid" };
  if (claims.iss !== ISSUER) return { ok: false, reason: "issuer" };
  const auds = Array.isArray(claims.aud) ? claims.aud : [claims.aud];
  if (!auds.includes(AUD)) return { ok: false, reason: "audience" };

  const email = String(claims.email || claims.common_name || "").toLowerCase();
  if (ALLOWED.length && !ALLOWED.includes(email)) return { ok: false, reason: "email" };
  return { ok: true, email: email || "(service token)" };
}

// --- Server ----------------------------------------------------------------
function tokenOf(req) {
  const header = req.headers["cf-access-jwt-assertion"];
  if (typeof header === "string" && header) return header;
  const cookie = req.headers.cookie || "";
  const m = /(?:^|;\s*)CF_Authorization=([^;]+)/.exec(cookie);
  return m ? decodeURIComponent(m[1]) : null;
}

const server = createServer(async (req, res) => {
  // Health check for the setup script / monitoring — says whether the gate is
  // configured, never anything about a request.
  if (req.url === "/health") {
    const body = JSON.stringify({
      configured: Boolean(TEAM && AUD),
      team: TEAM || null,
      keys: keys.size,
      restrictedTo: ALLOWED.length,
    });
    res.writeHead(200, { "content-type": "application/json" }).end(body);
    return;
  }

  if (!TEAM || !AUD) {
    res.writeHead(503, { "x-access-reason": "not-configured" }).end();
    return;
  }
  const token = tokenOf(req);
  if (!token) {
    res.writeHead(401, { "x-access-reason": "no-token" }).end();
    return;
  }
  try {
    const r = await verify(token);
    if (!r.ok) {
      res.writeHead(401, { "x-access-reason": r.reason }).end();
    } else if (req.url === "/whoami") {
      // Wer angemeldet ist — beantwortet NUR nach erfolgreicher Prüfung. nginx
      // reicht diese Anfrage in der content-Phase durch (siehe
      // dolphinclient-admin.conf), damit auth_request vorher wirklich läuft.
      res
        .writeHead(200, { "content-type": "application/json", "cache-control": "no-store" })
        .end(JSON.stringify({ email: r.email }));
    } else {
      res.writeHead(200, { "x-access-email": r.email }).end();
    }
  } catch (e) {
    // Cloudflare unreachable, clock skew, anything: closed, not open.
    res.writeHead(503, { "x-access-reason": `error:${String(e.message).slice(0, 40)}` }).end();
  }
});

server.listen(Number(PORT), HOST, () => {
  const state = TEAM && AUD ? `team=${TEAM}` : "NICHT KONFIGURIERT (alles 503)";
  console.log(`access-gate: http://${HOST}:${PORT} — ${state}`);
});

// Warm the key cache so the first real request is fast (failure is fine).
refreshKeys().catch(() => {});
