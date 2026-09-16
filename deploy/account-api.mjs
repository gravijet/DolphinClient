#!/usr/bin/env node
// The account API: email/password registration and login for
// dolphinclient.de, backing /login, /register and /dashboard on the
// website. Unlike admin-api.mjs (which is gated by Cloudflare Access
// behind nginx) this is public-facing — anyone may register — so it does
// its own password hashing, session issuance and rate limiting.
//
// State lives in one SQLite file (better-sqlite3, WAL). Sessions are a
// random 32-byte token in an HttpOnly cookie; only an HMAC digest of it is
// ever stored, so a stolen database dump cannot be replayed as a session
// the way a stored token could be.
//
//   ACCOUNT_API_LISTEN=127.0.0.1:8789        (optional)
//   ACCOUNT_SECRET=<random, stable across restarts>   (required — HMAC key)
//   ACCOUNT_DB=/var/lib/dolphinclient/account-api/account.db
//   ACCOUNT_CORS_ORIGIN=http://localhost:3000  (optional — dev only; prod
//     is same-origin behind nginx and needs no CORS header at all)
//   ACCOUNT_SMTP_HOST / _PORT / _USER / _PASS / _FROM   (optional — no
//     host set means password-reset mail is off, /auth/forgot says so)
import { createServer } from "node:http";
import { mkdirSync, chmodSync } from "node:fs";
import { dirname } from "node:path";
import { randomBytes, scryptSync, timingSafeEqual, createHmac } from "node:crypto";
import Database from "better-sqlite3";
import nodemailer from "nodemailer";

const [HOST, PORT] = (process.env.ACCOUNT_API_LISTEN || "127.0.0.1:8789").split(":");
const DB_PATH = process.env.ACCOUNT_DB || "/var/lib/dolphinclient/account-api/account.db";
const CORS_ORIGIN = process.env.ACCOUNT_CORS_ORIGIN || "";
const SECRET = process.env.ACCOUNT_SECRET;
if (!SECRET) {
  console.error("account-api: ACCOUNT_SECRET is not set — refusing to start with no HMAC key");
  process.exit(1);
}

// --- storage -----------------------------------------------------------
mkdirSync(dirname(DB_PATH), { recursive: true });
const db = new Database(DB_PATH);
db.pragma("journal_mode = WAL");
db.pragma("foreign_keys = ON");
try {
  chmodSync(DB_PATH, 0o600);
} catch {
  /* fine on a fresh file that doesn't exist yet under this exact path */
}

db.exec(`
  CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    display_name TEXT NOT NULL,
    minecraft_username TEXT,
    minecraft_uuid TEXT,
    created_at TEXT NOT NULL,
    last_login_at TEXT
  );
  CREATE TABLE IF NOT EXISTS sessions (
    token_digest TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    ip TEXT,
    user_agent TEXT
  );
  CREATE INDEX IF NOT EXISTS sessions_user ON sessions(user_id);
  CREATE TABLE IF NOT EXISTS password_resets (
    token_digest TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TEXT NOT NULL,
    used_at TEXT
  );
`);

const now = () => new Date().toISOString();

// --- passwords -----------------------------------------------------------
// scrypt, memory-hard, params baked into the stored string so they can
// change later without breaking existing hashes.
const SCRYPT_N = 16384;
const SCRYPT_R = 8;
const SCRYPT_P = 1;
const KEYLEN = 64;

function hashPassword(password) {
  const salt = randomBytes(16);
  const hash = scryptSync(password, salt, KEYLEN, { N: SCRYPT_N, r: SCRYPT_R, p: SCRYPT_P });
  return `scrypt$${SCRYPT_N}$${SCRYPT_R}$${SCRYPT_P}$${salt.toString("base64")}$${hash.toString("base64")}`;
}

// A fixed hash to verify against when no such user exists, so a login
// attempt for a nonexistent email takes the same scrypt-bound time as one
// for a real email with the wrong password — otherwise response time leaks
// which emails are registered.
const DUMMY_HASH = hashPassword(randomBytes(24).toString("hex"));

function verifyPassword(password, stored) {
  const parts = String(stored).split("$");
  if (parts.length !== 6 || parts[0] !== "scrypt") return false;
  const [, n, r, p, saltB64, hashB64] = parts;
  const salt = Buffer.from(saltB64, "base64");
  const expected = Buffer.from(hashB64, "base64");
  let actual;
  try {
    actual = scryptSync(password, salt, expected.length, { N: Number(n), r: Number(r), p: Number(p) });
  } catch {
    return false;
  }
  return actual.length === expected.length && timingSafeEqual(actual, expected);
}

// --- sessions --------------------------------------------------------------
const SESSION_MS = 30 * 24 * 3600 * 1000; // 30 days, no sliding renewal
const digest = (token) => createHmac("sha256", SECRET).update(token).digest("hex");
const newToken = () => randomBytes(32).toString("base64url");

function createSession(userId, req) {
  const token = newToken();
  const created = new Date();
  const expires = new Date(created.getTime() + SESSION_MS);
  db.prepare(
    `INSERT INTO sessions (token_digest, user_id, created_at, expires_at, ip, user_agent)
     VALUES (?, ?, ?, ?, ?, ?)`,
  ).run(digest(token), userId, created.toISOString(), expires.toISOString(), reqIp(req), uaOf(req));
  return token;
}

function sessionUser(token) {
  if (!token) return null;
  const row = db
    .prepare(
      `SELECT u.* FROM sessions s JOIN users u ON u.id = s.user_id
       WHERE s.token_digest = ? AND s.expires_at > ?`,
    )
    .get(digest(token), now());
  return row || null;
}

function destroySession(token) {
  db.prepare(`DELETE FROM sessions WHERE token_digest = ?`).run(digest(token));
}

function destroyAllSessions(userId) {
  db.prepare(`DELETE FROM sessions WHERE user_id = ?`).run(userId);
}

// A cheap sweep, not a scheduler dependency — one extra DELETE per hour of
// uptime is free, and an expired row left behind is only ever unreadable.
setInterval(() => {
  db.prepare(`DELETE FROM sessions WHERE expires_at <= ?`).run(now());
  db.prepare(`DELETE FROM password_resets WHERE expires_at <= ?`).run(now());
}, 3600_000).unref();

function uaOf(req) {
  return String(req.headers["user-agent"] || "").slice(0, 300);
}
function reqIp(req) {
  // Behind nginx; trust its forwarded header, not the raw socket peer.
  const fwd = String(req.headers["x-forwarded-for"] || "").split(",")[0].trim();
  return fwd || req.socket.remoteAddress || "";
}

// --- cookies -----------------------------------------------------------
const COOKIE = "dolphin_session";

function parseCookies(req) {
  const out = {};
  for (const part of String(req.headers.cookie || "").split(";")) {
    const i = part.indexOf("=");
    if (i === -1) continue;
    out[part.slice(0, i).trim()] = decodeURIComponent(part.slice(i + 1).trim());
  }
  return out;
}

function setSessionCookie(res, token) {
  const secure = process.env.NODE_ENV === "development" ? "" : "; Secure";
  res.setHeader(
    "Set-Cookie",
    `${COOKIE}=${token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=${Math.floor(SESSION_MS / 1000)}${secure}`,
  );
}

function clearSessionCookie(res) {
  const secure = process.env.NODE_ENV === "development" ? "" : "; Secure";
  res.setHeader("Set-Cookie", `${COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0${secure}`);
}

// --- validation ----------------------------------------------------------
// Everything here ends up in the database or an email; both are validated,
// not trusted.
const str = (s, max) => String(s ?? "").replace(/\r/g, "").trim().slice(0, max);

function cleanEmail(raw) {
  const e = str(raw, 254).toLowerCase();
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(e) ? e : null;
}

function cleanPassword(raw) {
  const p = String(raw ?? "");
  return p.length >= 8 && p.length <= 200 ? p : null;
}

function cleanDisplayName(raw) {
  const n = str(raw, 60);
  return n.length >= 1 ? n : null;
}

/** Mojang usernames: 3-16 chars, letters/digits/underscore. */
function cleanMcUsername(raw) {
  const n = str(raw, 16);
  return /^[A-Za-z0-9_]{3,16}$/.test(n) ? n : null;
}

// --- rate limiting -----------------------------------------------------
// In-memory, per-process — good enough for a single account-api instance
// behind nginx; the point is slowing down credential stuffing, not being a
// distributed limiter.
const attempts = new Map(); // key -> {count, resetAt}
const WINDOW_MS = 15 * 60_000;
const MAX_ATTEMPTS = 10;

function rateLimited(key) {
  const t = Date.now();
  const cur = attempts.get(key);
  if (!cur || cur.resetAt < t) {
    attempts.set(key, { count: 1, resetAt: t + WINDOW_MS });
    return false;
  }
  cur.count += 1;
  return cur.count > MAX_ATTEMPTS;
}
// Forget stale entries so this map doesn't grow forever.
setInterval(() => {
  const t = Date.now();
  for (const [k, v] of attempts) if (v.resetAt < t) attempts.delete(k);
}, 600_000).unref();

// --- mail (optional) -----------------------------------------------------
const SMTP_HOST = process.env.ACCOUNT_SMTP_HOST || "";
const mailConfigured = () => Boolean(SMTP_HOST);
const transporter = mailConfigured()
  ? nodemailer.createTransport({
      host: SMTP_HOST,
      port: Number(process.env.ACCOUNT_SMTP_PORT || 587),
      secure: Number(process.env.ACCOUNT_SMTP_PORT) === 465,
      auth: process.env.ACCOUNT_SMTP_USER
        ? { user: process.env.ACCOUNT_SMTP_USER, pass: process.env.ACCOUNT_SMTP_PASS }
        : undefined,
    })
  : null;

async function sendMail(to, subject, text) {
  if (!transporter) return false;
  try {
    await transporter.sendMail({
      from: process.env.ACCOUNT_SMTP_FROM || "DolphinClient <no-reply@dolphinclient.de>",
      to,
      subject,
      text,
    });
    return true;
  } catch (e) {
    console.error(`account-api: mail to ${to} failed: ${e.message}`);
    return false;
  }
}

// --- Mojang lookup (optional profile field) -------------------------------
async function mojangLookup(username) {
  const res = await fetch(
    `https://api.mojang.com/users/profiles/minecraft/${encodeURIComponent(username)}`,
    { signal: AbortSignal.timeout(5000) },
  );
  if (res.status === 204 || res.status === 404) return null;
  if (!res.ok) throw new Error(`mojang ${res.status}`);
  const body = await res.json();
  return { name: body.name, uuid: body.id };
}

// --- shapes returned to the client ----------------------------------------
function publicUser(u) {
  return {
    id: u.id,
    email: u.email,
    display_name: u.display_name,
    minecraft_username: u.minecraft_username,
    minecraft_uuid: u.minecraft_uuid,
    created_at: u.created_at,
    last_login_at: u.last_login_at,
  };
}

// --- HTTP ------------------------------------------------------------------
const json = (res, code, body, extraHeaders) =>
  res
    .writeHead(code, { "content-type": "application/json", "cache-control": "no-store", ...extraHeaders })
    .end(JSON.stringify(body));

function readBody(req, limit = 64 * 1024) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let size = 0;
    req.on("data", (c) => {
      size += c.length;
      if (size > limit) {
        reject(new Error("too large"));
        req.destroy();
        return;
      }
      chunks.push(c);
    });
    req.on("end", () => resolve(Buffer.concat(chunks)));
    req.on("error", reject);
  });
}

async function readJson(req) {
  const buf = await readBody(req);
  if (!buf.length) return {};
  return JSON.parse(buf.toString("utf8"));
}

function corsHeaders(req) {
  if (!CORS_ORIGIN) return {};
  const origin = req.headers.origin;
  if (origin !== CORS_ORIGIN) return {};
  return {
    "access-control-allow-origin": origin,
    "access-control-allow-credentials": "true",
  };
}

const server = createServer(async (req, res) => {
  const cors = corsHeaders(req);
  for (const [k, v] of Object.entries(cors)) res.setHeader(k, v);

  if (req.method === "OPTIONS") {
    res.setHeader("access-control-allow-methods", "GET, POST, PATCH, DELETE, OPTIONS");
    res.setHeader("access-control-allow-headers", "content-type");
    res.writeHead(204).end();
    return;
  }

  const url = new URL(req.url, "http://api");
  const path = url.pathname.replace(/\/+$/, "") || "/";
  const method = req.method || "GET";

  if (path === "/health") {
    return json(res, 200, { ok: true, mail: mailConfigured() });
  }

  try {
    // POST /auth/register — create an account, sign it in immediately.
    if (method === "POST" && path === "/auth/register") {
      const ip = reqIp(req);
      if (rateLimited(`register:${ip}`)) return json(res, 429, { error: "too many attempts, try later" });
      const body = await readJson(req);
      const email = cleanEmail(body.email);
      const password = cleanPassword(body.password);
      const displayName = cleanDisplayName(body.display_name || body.email);
      if (!email) return json(res, 400, { error: "that doesn't look like an email address" });
      if (!password) return json(res, 400, { error: "password must be 8-200 characters" });
      if (!displayName) return json(res, 400, { error: "display name is required" });
      let info;
      try {
        info = db
          .prepare(
            `INSERT INTO users (email, password_hash, display_name, created_at) VALUES (?, ?, ?, ?)`,
          )
          .run(email, hashPassword(password), displayName, now());
      } catch (e) {
        if (e.code === "SQLITE_CONSTRAINT_UNIQUE" || e.code === "SQLITE_CONSTRAINT") {
          return json(res, 409, { error: "an account with that email already exists" });
        }
        throw e;
      }
      const token = createSession(info.lastInsertRowid, req);
      setSessionCookie(res, token);
      const user = db.prepare(`SELECT * FROM users WHERE id = ?`).get(info.lastInsertRowid);
      return json(res, 201, { user: publicUser(user) });
    }

    // POST /auth/login
    if (method === "POST" && path === "/auth/login") {
      const ip = reqIp(req);
      if (rateLimited(`login:${ip}`)) return json(res, 429, { error: "too many attempts, try later" });
      const body = await readJson(req);
      const email = cleanEmail(body.email);
      const password = String(body.password ?? "");
      const user = email ? db.prepare(`SELECT * FROM users WHERE email = ?`).get(email) : null;
      // Always run the scrypt verify, even against a dummy hash for an
      // unknown email, so response time doesn't reveal which emails exist.
      const passwordOk = verifyPassword(password, user ? user.password_hash : DUMMY_HASH);
      if (!user || !passwordOk) {
        return json(res, 401, { error: "wrong email or password" });
      }
      db.prepare(`UPDATE users SET last_login_at = ? WHERE id = ?`).run(now(), user.id);
      const token = createSession(user.id, req);
      setSessionCookie(res, token);
      return json(res, 200, { user: publicUser(user) });
    }

    // Everything past this point needs a session; resolve it once.
    const cookies = parseCookies(req);
    const sessionToken = cookies[COOKIE];
    const user = sessionUser(sessionToken);

    // POST /auth/logout — end this one session.
    if (method === "POST" && path === "/auth/logout") {
      if (sessionToken) destroySession(sessionToken);
      clearSessionCookie(res);
      return json(res, 200, { ok: true });
    }

    // POST /auth/logout-all — end every session for this account.
    if (method === "POST" && path === "/auth/logout-all") {
      if (!user) return json(res, 401, { error: "not signed in" });
      destroyAllSessions(user.id);
      clearSessionCookie(res);
      return json(res, 200, { ok: true });
    }

    // POST /auth/forgot — always answers the same way whether or not the
    // email exists, so this cannot be used to test which emails are
    // registered.
    if (method === "POST" && path === "/auth/forgot") {
      const ip = reqIp(req);
      if (rateLimited(`forgot:${ip}`)) return json(res, 429, { error: "too many attempts, try later" });
      if (!mailConfigured()) {
        return json(res, 503, { error: "password reset by email is not set up on this server" });
      }
      const body = await readJson(req);
      const email = cleanEmail(body.email);
      const target = email ? db.prepare(`SELECT * FROM users WHERE email = ?`).get(email) : null;
      if (target) {
        const token = newToken();
        const expires = new Date(Date.now() + 3600_000).toISOString();
        db.prepare(
          `INSERT INTO password_resets (token_digest, user_id, expires_at) VALUES (?, ?, ?)`,
        ).run(digest(token), target.id, expires);
        const link = `https://dolphinclient.de/reset?token=${token}`;
        await sendMail(
          target.email,
          "Reset your DolphinClient password",
          `Someone asked to reset the password for this account.\n\nIf that was you: ${link}\n(valid for one hour)\n\nIf it wasn't you, ignore this email — your password is unchanged.`,
        );
      }
      return json(res, 200, { ok: true, message: "if that email exists, a reset link was sent" });
    }

    // POST /auth/reset — apply a new password from a forgot-password token.
    if (method === "POST" && path === "/auth/reset") {
      const body = await readJson(req);
      const token = str(body.token, 200);
      const password = cleanPassword(body.password);
      if (!token || !password) return json(res, 400, { error: "missing token or bad password" });
      const row = db
        .prepare(
          `SELECT * FROM password_resets WHERE token_digest = ? AND used_at IS NULL AND expires_at > ?`,
        )
        .get(digest(token), now());
      if (!row) return json(res, 400, { error: "that reset link is invalid or has expired" });
      db.prepare(`UPDATE users SET password_hash = ? WHERE id = ?`).run(hashPassword(password), row.user_id);
      db.prepare(`UPDATE password_resets SET used_at = ? WHERE token_digest = ?`).run(now(), row.token_digest);
      destroyAllSessions(row.user_id); // a reset invalidates every existing session
      return json(res, 200, { ok: true });
    }

    // Everything below requires a signed-in session.
    if (!user) return json(res, 401, { error: "not signed in" });

    // GET /me
    if (method === "GET" && path === "/me") {
      return json(res, 200, { user: publicUser(user) });
    }

    // PATCH /profile — display name and/or Minecraft username.
    if (method === "PATCH" && path === "/profile") {
      const body = await readJson(req);
      const sets = [];
      const vals = [];
      if (body.display_name !== undefined) {
        const n = cleanDisplayName(body.display_name);
        if (!n) return json(res, 400, { error: "display name is required" });
        sets.push("display_name = ?");
        vals.push(n);
      }
      if (body.minecraft_username !== undefined) {
        if (body.minecraft_username === null || body.minecraft_username === "") {
          sets.push("minecraft_username = NULL", "minecraft_uuid = NULL");
        } else {
          const n = cleanMcUsername(body.minecraft_username);
          if (!n) return json(res, 400, { error: "that isn't a valid Minecraft username" });
          let profile;
          try {
            profile = await mojangLookup(n);
          } catch {
            return json(res, 502, { error: "couldn't reach Mojang right now, try again" });
          }
          if (!profile) return json(res, 400, { error: "no Minecraft account with that username" });
          sets.push("minecraft_username = ?", "minecraft_uuid = ?");
          vals.push(profile.name, profile.uuid);
        }
      }
      if (!sets.length) return json(res, 400, { error: "nothing to update" });
      vals.push(user.id);
      db.prepare(`UPDATE users SET ${sets.join(", ")} WHERE id = ?`).run(...vals);
      const fresh = db.prepare(`SELECT * FROM users WHERE id = ?`).get(user.id);
      return json(res, 200, { user: publicUser(fresh) });
    }

    // POST /auth/change-password — while signed in, knowing the old one.
    if (method === "POST" && path === "/auth/change-password") {
      const body = await readJson(req);
      if (!verifyPassword(String(body.current_password ?? ""), user.password_hash)) {
        return json(res, 401, { error: "current password is wrong" });
      }
      const next = cleanPassword(body.new_password);
      if (!next) return json(res, 400, { error: "new password must be 8-200 characters" });
      db.prepare(`UPDATE users SET password_hash = ? WHERE id = ?`).run(hashPassword(next), user.id);
      // Keep the session that just proved itself; drop every other one.
      db.prepare(`DELETE FROM sessions WHERE user_id = ? AND token_digest != ?`).run(
        user.id,
        digest(sessionToken),
      );
      return json(res, 200, { ok: true });
    }

    // DELETE /me — delete the account and everything tied to it.
    if (method === "DELETE" && path === "/me") {
      db.prepare(`DELETE FROM users WHERE id = ?`).run(user.id); // cascades sessions/resets
      clearSessionCookie(res);
      return json(res, 200, { ok: true });
    }

    return json(res, 404, { error: `no route for ${method} ${path}` });
  } catch (e) {
    const msg = String(e?.message || e);
    const code = msg === "too large" ? 413 : msg.includes("JSON") ? 400 : 500;
    console.error(`account-api: ${method} ${path} failed: ${msg}`);
    return json(res, code, { error: code === 500 ? "internal error" : msg });
  }
});

server.listen(Number(PORT), HOST, () => {
  console.log(
    `account-api: http://${HOST}:${PORT} — db ${DB_PATH}, mail ${mailConfigured() ? "configured" : "off"}`,
  );
});
