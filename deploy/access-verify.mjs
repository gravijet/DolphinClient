// Cloudflare Access token verification — the one place that decides whether a
// request carries a real, current, in-audience identity.
//
// Both origin-side services import this: `access-gate.mjs` (the `auth_request`
// backend nginx asks on every request) and `admin-api.mjs` (the write API for
// the changelog). Sharing one implementation means there is exactly one set of
// rules to get right, and `test-access-gate.mjs` exercises them for both.
//
// Configuration comes from the environment (/etc/dolphinclient/access.env):
//   ACCESS_TEAM_DOMAIN=yourteam.cloudflareaccess.com
//   ACCESS_AUD=<the Access application's Application Audience (AUD) tag>
//   ACCESS_ALLOWED_EMAILS=you@example.com,user@example.invalid  (optional)
//
// With no team/AUD configured nothing verifies — callers must answer 503.
import { createPublicKey, createVerify } from "node:crypto";

export const TEAM = (process.env.ACCESS_TEAM_DOMAIN || "")
  .trim()
  .replace(/^https?:\/\//, "")
  .replace(/\/$/, "");
export const AUD = (process.env.ACCESS_AUD || "").trim();
export const ALLOWED = (process.env.ACCESS_ALLOWED_EMAILS || "")
  .split(",")
  .map((s) => s.trim().toLowerCase())
  .filter(Boolean);

// ACCESS_CERTS_URL overrides the key source — only for the self-test
// (deploy/test-access-gate.mjs). In production it stays empty.
const CERTS_URL =
  process.env.ACCESS_CERTS_URL || (TEAM ? `https://${TEAM}/cdn-cgi/access/certs` : null);
const ISSUER = TEAM ? `https://${TEAM}` : null;

export const configured = () => Boolean(TEAM && AUD);

// --- key cache -------------------------------------------------------------
let keys = new Map(); // kid -> KeyObject
let keysFetched = 0;
let inflight = null;

export function keyCount() {
  return keys.size;
}

export async function refreshKeys(force = false) {
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
export async function verify(token) {
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
  if (typeof claims.nbf === "number" && claims.nbf > now + 30)
    return { ok: false, reason: "not-yet-valid" };
  if (claims.iss !== ISSUER) return { ok: false, reason: "issuer" };
  const auds = Array.isArray(claims.aud) ? claims.aud : [claims.aud];
  if (!auds.includes(AUD)) return { ok: false, reason: "audience" };

  const email = String(claims.email || claims.common_name || "").toLowerCase();
  if (ALLOWED.length && !ALLOWED.includes(email)) return { ok: false, reason: "email" };
  return { ok: true, email: email || "(service token)" };
}

/** The Access token on a request: the header Cloudflare sets, or its cookie. */
export function tokenOf(req) {
  const header = req.headers["cf-access-jwt-assertion"];
  if (typeof header === "string" && header) return header;
  const cookie = req.headers.cookie || "";
  const m = /(?:^|;\s*)CF_Authorization=([^;]+)/.exec(cookie);
  return m ? decodeURIComponent(m[1]) : null;
}

/**
 * Guard for a service endpoint: answers the request itself and returns null
 * when the caller must not proceed, or {email} when it may.
 *
 * Unconfigured is 503, not 401 — "we cannot check" must never read as "you are
 * not allowed", and it must never read as "come in" either.
 */
export async function requireIdentity(req, res) {
  if (!configured()) {
    res.writeHead(503, { "x-access-reason": "not-configured" }).end();
    return null;
  }
  const token = tokenOf(req);
  if (!token) {
    res.writeHead(401, { "x-access-reason": "no-token" }).end();
    return null;
  }
  try {
    const r = await verify(token);
    if (!r.ok) {
      res.writeHead(401, { "x-access-reason": r.reason }).end();
      return null;
    }
    return { email: r.email };
  } catch (e) {
    // Cloudflare unreachable, clock skew, anything: closed, not open.
    res.writeHead(503, { "x-access-reason": `error:${String(e.message).slice(0, 40)}` }).end();
    return null;
  }
}
