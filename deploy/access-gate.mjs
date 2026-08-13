#!/usr/bin/env node
// Origin gate for the admin portal ("zero trust" means the origin trusts
// nobody either). Cloudflare Access authenticates at the edge and attaches a
// signed JWT (`Cf-Access-Jwt-Assertion`, or the `CF_Authorization` cookie).
// This tiny service verifies that token — signature, issuer, audience,
// lifetime and optionally the e-mail address — and answers nginx's
// `auth_request` with 200 or 401.
//
// Without a valid token NOBODY reaches /admin: bypassing the Cloudflare
// address and talking to the origin directly means no token, and out you go.
// If the configuration is missing the service answers 503 — closed when in
// doubt, never open.
//
// The rules themselves live in access-verify.mjs, shared with admin-api.mjs so
// there is exactly one implementation to get right.
//
// Configuration (/etc/dolphinclient/access.env):
//   ACCESS_TEAM_DOMAIN=yourteam.cloudflareaccess.com
//   ACCESS_AUD=<the Access application's Application Audience (AUD) tag>
//   ACCESS_ALLOWED_EMAILS=you@example.com,second@example.com  (optional)
//   ACCESS_LISTEN=127.0.0.1:8787                              (optional)
//
// Started by the systemd unit `dolphinclient-access.service` (see
// setup-zero-trust.sh). Note: that unit reads access.env once, at start — the
// setup script therefore restarts it whenever the values change.
import { createServer } from "node:http";
import {
  ALLOWED,
  TEAM,
  AUD,
  configured,
  keyCount,
  refreshKeys,
  requireIdentity,
} from "./access-verify.mjs";

const [HOST, PORT] = (process.env.ACCESS_LISTEN || "127.0.0.1:8787").split(":");

const server = createServer(async (req, res) => {
  // Health check for the setup script / monitoring — says whether the gate is
  // configured, never anything about a request.
  if (req.url === "/health") {
    const body = JSON.stringify({
      configured: configured(),
      team: TEAM || null,
      keys: keyCount(),
      restrictedTo: ALLOWED.length,
    });
    res.writeHead(200, { "content-type": "application/json" }).end(body);
    return;
  }

  const who = await requireIdentity(req, res);
  if (!who) return; // already answered: 401 (no/invalid token) or 503

  if (req.url === "/whoami") {
    // Who is signed in — answered ONLY after a successful check. nginx
    // forwards this request in the content phase (see
    // dolphinclient-admin.conf) so that auth_request really runs first.
    res
      .writeHead(200, { "content-type": "application/json", "cache-control": "no-store" })
      .end(JSON.stringify({ email: who.email }));
    return;
  }
  res.writeHead(200, { "x-access-email": who.email }).end();
});

server.listen(Number(PORT), HOST, () => {
  const state = TEAM && AUD ? `team=${TEAM}` : "NOT CONFIGURED (everything 503)";
  console.log(`access-gate: http://${HOST}:${PORT} — ${state}`);
});

// Warm the key cache so the first real request is fast (failure is fine).
refreshKeys().catch(() => {});
