#!/usr/bin/env node
// Self-test for the account API (deploy/account-api.mjs).
//
// Runs the real server against a throwaway SQLite file in /tmp — nothing here
// touches the live database. Checks the golden path (register, sign in, use
// the session, sign out) and, more importantly, what must be refused:
// duplicate emails, wrong passwords, a weak password, an invalid Minecraft
// username, and every protected route without a session.
//
// The Minecraft username *success* path (a real Mojang API lookup) is
// exercised manually, not here — a unit test should not depend on a network
// call to a third party to pass.
//
//   node deploy/test-account-api.mjs
import { spawn } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import Database from "better-sqlite3";

const API = fileURLToPath(new URL("./account-api.mjs", import.meta.url));
const PORT = 8796;
const DB = join(mkdtempSync(join(tmpdir(), "dolphin-account-test-")), "account.db");

const api = spawn(process.execPath, [API], {
  env: {
    ...process.env,
    ACCOUNT_SECRET: "selftest-secret-not-for-production",
    ACCOUNT_API_LISTEN: `127.0.0.1:${PORT}`,
    ACCOUNT_DB: DB,
    NODE_ENV: "development", // so the session cookie isn't marked Secure over plain http
  },
  stdio: ["ignore", "ignore", "inherit"],
});
process.on("exit", () => api.kill());

for (let i = 0; i < 50; i++) {
  try {
    await fetch(`http://127.0.0.1:${PORT}/health`);
    break;
  } catch {
    await new Promise((r) => setTimeout(r, 100));
  }
}

let failed = 0;
function check(name, ok, detail = "") {
  if (!ok) failed++;
  console.log(`${ok ? "  ok  " : "FAIL  "}${name}${detail ? `: ${detail}` : ""}`);
}

/** A tiny cookie jar — just enough to carry one session cookie between calls. */
let cookie = "";
async function call(method, path, body) {
  const headers = {};
  if (body !== undefined) headers["content-type"] = "application/json";
  if (cookie) headers.cookie = cookie;
  const res = await fetch(`http://127.0.0.1:${PORT}${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const setCookie = res.headers.get("set-cookie");
  if (setCookie) cookie = setCookie.split(";")[0];
  const parsed = await res.json().catch(() => null);
  return { status: res.status, body: parsed };
}

const email = `test-${Date.now()}@example.com`;

// --- registration ---------------------------------------------------------
{
  const r = await call("POST", "/auth/register", { email, password: "hunter22", display_name: "Tester" });
  check("register succeeds", r.status === 201 && r.body?.user?.email === email);
  check("register sets a session cookie", cookie.startsWith("dolphin_session="));
  check(
    "register auto-verifies when no mail server is configured",
    r.body?.user?.email_verified === true,
  );
}
{
  const r = await call("POST", "/auth/register", { email, password: "hunter22", display_name: "Tester" });
  check("register rejects a duplicate email", r.status === 409);
}
{
  const r = await call("POST", "/auth/register", { email: "not-an-email", password: "hunter22", display_name: "x" });
  check("register rejects a bad email", r.status === 400);
}
{
  const r = await call("POST", "/auth/register", { email: "another@example.com", password: "short", display_name: "x" });
  check("register rejects a short password", r.status === 400);
}

// --- session use -----------------------------------------------------------
{
  const r = await call("GET", "/me");
  check("me works while signed in", r.status === 200 && r.body?.user?.email === email);
}
{
  const r = await call("PATCH", "/profile", { minecraft_username: "no way this is valid" });
  check("profile rejects a bad Minecraft username", r.status === 400);
}
{
  const r = await call("POST", "/auth/forgot", { email });
  check("forgot answers 503 when mail is not configured", r.status === 503);
}
{
  const r = await call("POST", "/auth/resend-verification");
  check(
    "resend-verification is a no-op once already verified",
    r.status === 200 && r.body?.already_verified === true,
  );
}
{
  const r = await call("POST", "/auth/verify", { token: "not-a-real-token" });
  check("verify rejects an invalid token", r.status === 400);
}

// --- sessions ---------------------------------------------------------------
{
  const r = await call("GET", "/sessions");
  check(
    "sessions lists the current session, marked current",
    r.status === 200 && r.body?.sessions?.length === 1 && r.body.sessions[0].current === true,
  );
}
{
  const r = await call("DELETE", "/sessions/not-a-real-session-id");
  check("revoking a session that isn't yours (or doesn't exist) 404s", r.status === 404);
}

// --- logout / login ---------------------------------------------------------
{
  const r = await call("POST", "/auth/logout");
  check("logout succeeds", r.status === 200);
}
{
  const r = await call("GET", "/me");
  check("me is refused after logout", r.status === 401);
}
{
  const r = await call("POST", "/auth/login", { email, password: "wrong" });
  check("login rejects the wrong password", r.status === 401);
}
{
  const r = await call("POST", "/auth/login", { email, password: "hunter22" });
  check("login succeeds with the right password", r.status === 200 && r.body?.user?.email === email);
}

// --- revoking another session -----------------------------------------------
{
  // A second, independent login (its own cookie jar) so there's a session
  // to revoke that isn't the one making the request.
  const other = { cookie: "" };
  const otherLogin = await fetch(`http://127.0.0.1:${PORT}/auth/login`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ email, password: "hunter22" }),
  });
  other.cookie = (otherLogin.headers.get("set-cookie") || "").split(";")[0];

  const list = await call("GET", "/sessions");
  check("sessions now lists both logins", list.body?.sessions?.length === 2);
  const otherSession = list.body.sessions.find((s) => !s.current);
  const revoke = await call("DELETE", `/sessions/${otherSession.id}`);
  check("revoking the other session succeeds", revoke.status === 200);

  const stillWorks = await fetch(`http://127.0.0.1:${PORT}/me`, { headers: { cookie: other.cookie } });
  check("the revoked session can no longer authenticate", stillWorks.status === 401);
}

// --- unauthenticated access to everything protected -------------------------
cookie = "";
for (const [method, path] of [
  ["GET", "/me"],
  ["PATCH", "/profile"],
  ["POST", "/auth/logout-all"],
  ["POST", "/auth/change-password"],
  ["DELETE", "/me"],
  ["GET", "/sessions"],
  ["DELETE", "/sessions/x"],
  ["POST", "/auth/resend-verification"],
]) {
  const r = await call(method, path, method === "GET" ? undefined : {});
  check(`${method} ${path} refused without a session`, r.status === 401);
}

// --- ban/unban functionality -----------------------------------------------
{
  const bannedEmail = `banned-${Date.now()}@example.com`;
  const regResp = await call("POST", "/auth/register", {
    email: bannedEmail,
    password: "hunter22",
    display_name: "ToBeBanned",
  });
  check("can register a user to be banned", regResp.status === 201);

  // Simulate admin ban by setting banned_at in the database
  const db = new Database(DB);
  const user = db.prepare("SELECT id FROM users WHERE email = ?").get(bannedEmail);
  db.prepare("UPDATE users SET banned_at = ? WHERE id = ?").run(new Date().toISOString(), user.id);
  db.close();

  // Logout first
  await call("POST", "/auth/logout");
  cookie = "";

  // Try to login — should fail with 403
  const loginResp = await call("POST", "/auth/login", {
    email: bannedEmail,
    password: "hunter22",
  });
  check("login fails with 403 for a banned account", loginResp.status === 403 && loginResp.body?.error?.includes("banned"));
}

console.log(failed ? `\n${failed} test(s) FAILED` : "\nAll tests passed.");
process.exit(failed ? 1 : 0);
