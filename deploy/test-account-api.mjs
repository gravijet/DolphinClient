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

// --- unauthenticated access to everything protected -------------------------
cookie = "";
for (const [method, path] of [
  ["GET", "/me"],
  ["PATCH", "/profile"],
  ["POST", "/auth/logout-all"],
  ["POST", "/auth/change-password"],
  ["DELETE", "/me"],
]) {
  const r = await call(method, path, method === "GET" ? undefined : {});
  check(`${method} ${path} refused without a session`, r.status === 401);
}

console.log(failed ? `\n${failed} test(s) FAILED` : "\nAll tests passed.");
process.exit(failed ? 1 : 0);
