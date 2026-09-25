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
//     host set means password-reset and verification mail are off; a fresh
//     account is then marked verified immediately since there's no way to
//     prove the address, and /auth/forgot says plainly that reset is off)
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
    last_login_at TEXT,
    email_verified_at TEXT,
    banned_at TEXT
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
  CREATE TABLE IF NOT EXISTS email_verifications (
    token_digest TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TEXT NOT NULL
  );
  CREATE INDEX IF NOT EXISTS email_verifications_user ON email_verifications(user_id);
  CREATE TABLE IF NOT EXISTS login_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    ip TEXT,
    user_agent TEXT,
    success INTEGER NOT NULL,
    reason TEXT,
    country TEXT,
    city TEXT
  );
  CREATE INDEX IF NOT EXISTS login_history_user ON login_history(user_id, created_at DESC);
  CREATE TABLE IF NOT EXISTS totp_pending (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    secret TEXT NOT NULL,
    created_at TEXT NOT NULL
  );
  CREATE TABLE IF NOT EXISTS totp_challenges (
    token_digest TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TEXT NOT NULL
  );
`);
// A database from before email verification existed won't have the column —
// add it in place rather than requiring a manual migration step. Existing
// accounts are treated as already verified (they predate the feature and
// have been logging in for a while), so this never locks anyone out.
if (!db.prepare(`PRAGMA table_info(users)`).all().some((c) => c.name === "email_verified_at")) {
  db.exec(`ALTER TABLE users ADD COLUMN email_verified_at TEXT`);
  db.prepare(`UPDATE users SET email_verified_at = created_at WHERE email_verified_at IS NULL`).run();
}

// A database from before account bans existed won't have the column — add it
// in place. Existing accounts default to not banned (banned_at = NULL).
if (!db.prepare(`PRAGMA table_info(users)`).all().some((c) => c.name === "banned_at")) {
  db.exec(`ALTER TABLE users ADD COLUMN banned_at TEXT`);
}

// A database from before profile customization / 2FA existed won't have
// these columns — add each in place. All default to unset/disabled, so an
// existing account is unaffected until its owner opts in.
{
  const cols = db.prepare(`PRAGMA table_info(users)`).all().map((c) => c.name);
  const addColumn = (name, type) => {
    if (!cols.includes(name)) db.exec(`ALTER TABLE users ADD COLUMN ${name} ${type}`);
  };
  addColumn("bio", "TEXT");
  addColumn("avatar_url", "TEXT");
  addColumn("social_links", "TEXT");
  addColumn("totp_secret", "TEXT");
  addColumn("totp_enabled_at", "TEXT");
  addColumn("totp_backup_codes", "TEXT");
}

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

// --- TOTP two-factor auth (RFC 6238), no external dependency ---------------
// A standard 6-digit, 30-second TOTP over HMAC-SHA1, compatible with every
// authenticator app (Google Authenticator, Authy, 1Password, ...). Base32 is
// hand-rolled since Node has no built-in codec for it and the alphabet is
// tiny — not worth a dependency for.
const BASE32_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

function base32Encode(buf) {
  let bits = "";
  for (const byte of buf) bits += byte.toString(2).padStart(8, "0");
  let out = "";
  for (let i = 0; i + 5 <= bits.length; i += 5) out += BASE32_ALPHABET[parseInt(bits.slice(i, i + 5), 2)];
  const rem = bits.length % 5;
  if (rem) out += BASE32_ALPHABET[parseInt(bits.slice(bits.length - rem).padEnd(5, "0"), 2)];
  return out;
}

function base32Decode(str) {
  const clean = String(str).toUpperCase().replace(/[^A-Z2-7]/g, "");
  let bits = "";
  for (const ch of clean) {
    const v = BASE32_ALPHABET.indexOf(ch);
    if (v === -1) continue;
    bits += v.toString(2).padStart(5, "0");
  }
  const bytes = [];
  for (let i = 0; i + 8 <= bits.length; i += 8) bytes.push(parseInt(bits.slice(i, i + 8), 2));
  return Buffer.from(bytes);
}

const generateTotpSecret = () => base32Encode(randomBytes(20)); // 160-bit, the RFC 4226 default

function hotp(secret, counter) {
  const key = base32Decode(secret);
  const buf = Buffer.alloc(8);
  buf.writeBigInt64BE(BigInt(counter));
  const hmac = createHmac("sha1", key).update(buf).digest();
  const offset = hmac[hmac.length - 1] & 0x0f;
  const code =
    ((hmac[offset] & 0x7f) << 24) |
    ((hmac[offset + 1] & 0xff) << 16) |
    ((hmac[offset + 2] & 0xff) << 8) |
    (hmac[offset + 3] & 0xff);
  return String(code % 1_000_000).padStart(6, "0");
}

/** Accepts a code from one step before/after now, to tolerate clock drift. */
function verifyTotp(secret, token) {
  const clean = String(token ?? "").replace(/\s/g, "");
  if (!/^\d{6}$/.test(clean)) return false;
  const counter = Math.floor(Date.now() / 30000);
  for (const w of [0, -1, 1]) {
    if (timingSafeStrEqual(hotp(secret, counter + w), clean)) return true;
  }
  return false;
}

function timingSafeStrEqual(a, b) {
  const bufA = Buffer.from(String(a));
  const bufB = Buffer.from(String(b));
  return bufA.length === bufB.length && timingSafeEqual(bufA, bufB);
}

function totpAuthUrl(email, secret) {
  const label = encodeURIComponent(`DolphinClient:${email}`);
  return `otpauth://totp/${label}?secret=${secret}&issuer=DolphinClient&algorithm=SHA1&digits=6&period=30`;
}

/** 10 backup codes, formatted XXXX-XXXX. Returned raw exactly once; only
 * their digests (same HMAC as session tokens) are ever stored. */
function generateBackupCodes() {
  const codes = [];
  for (let i = 0; i < 10; i++) {
    const raw = randomBytes(5).toString("hex").toUpperCase(); // 10 hex chars
    codes.push(`${raw.slice(0, 5)}-${raw.slice(5)}`);
  }
  return codes;
}

function hashBackupCodes(codes) {
  return JSON.stringify(codes.map((c) => ({ hash: digest(c.toUpperCase()), used_at: null })));
}

function consumeBackupCode(user, code) {
  const clean = String(code ?? "").trim().toUpperCase();
  if (!clean || !user.totp_backup_codes) return false;
  let list;
  try {
    list = JSON.parse(user.totp_backup_codes);
  } catch {
    return false;
  }
  const h = digest(clean);
  const entry = list.find((c) => c.hash === h && !c.used_at);
  if (!entry) return false;
  entry.used_at = now();
  db.prepare(`UPDATE users SET totp_backup_codes = ? WHERE id = ?`).run(JSON.stringify(list), user.id);
  return true;
}

// --- login history / geoip (best-effort, never blocks the response) -------
// Private/loopback ranges never resolve to anything useful, so skip the
// network round-trip for them entirely.
function isPrivateIp(ip) {
  return (
    !ip ||
    ip === "::1" ||
    /^127\./.test(ip) ||
    /^10\./.test(ip) ||
    /^192\.168\./.test(ip) ||
    /^172\.(1[6-9]|2\d|3[01])\./.test(ip)
  );
}

function recordLogin(userId, req, success, reason) {
  const info = db
    .prepare(
      `INSERT INTO login_history (user_id, created_at, ip, user_agent, success, reason)
       VALUES (?, ?, ?, ?, ?, ?)`,
    )
    .run(userId, now(), reqIp(req), uaOf(req), success ? 1 : 0, reason);
  const rowId = info.lastInsertRowid;
  const ip = reqIp(req);
  if (isPrivateIp(ip)) return;
  // Fire-and-forget: geolocation is a nice-to-have for the security page,
  // never something the login flow should wait on or fail over.
  fetch(`http://ip-api.com/json/${encodeURIComponent(ip)}?fields=status,country,city`, {
    signal: AbortSignal.timeout(2000),
  })
    .then((r) => (r.ok ? r.json() : null))
    .then((geo) => {
      if (geo?.status === "success") {
        db.prepare(`UPDATE login_history SET country = ?, city = ? WHERE id = ?`).run(
          geo.country || null,
          geo.city || null,
          rowId,
        );
      }
    })
    .catch(() => {
      /* best-effort — an unreachable geoip service never affects login */
    });
}

// A sweep for expired TOTP login challenges, same pattern as sessions.
setInterval(() => {
  db.prepare(`DELETE FROM totp_challenges WHERE expires_at <= ?`).run(now());
}, 3600_000).unref();

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

/** Plain text, no length limit beyond what fits on a profile card. */
function cleanBio(raw) {
  return str(raw, 500);
}

/** Must be an http(s) URL, nothing else — this is rendered as an <img src>. */
function cleanAvatarUrl(raw) {
  const u = str(raw, 500);
  if (!u) return "";
  try {
    const parsed = new URL(u);
    return parsed.protocol === "http:" || parsed.protocol === "https:" ? u : null;
  } catch {
    return null;
  }
}

const SOCIAL_KEYS = ["twitter", "discord", "github", "youtube"];

/** `{twitter, discord, github, youtube}` — plain handles/names, not URLs
 * (the frontend builds the link), each capped to a sane handle length. */
function cleanSocialLinks(raw) {
  if (raw === null) return "{}";
  if (typeof raw !== "object") return null;
  const out = {};
  for (const key of SOCIAL_KEYS) {
    if (raw[key] === undefined || raw[key] === null || raw[key] === "") continue;
    const v = str(raw[key], 80).replace(/^@/, "");
    // "#" allowed for old-style Discord discriminators (name#1234).
    if (!/^[A-Za-z0-9._#-]{1,80}$/.test(v)) return null;
    out[key] = v;
  }
  return JSON.stringify(out);
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

const VERIFY_MS = 24 * 3600_000; // a verification link lives a day, longer than a reset link

async function sendVerificationEmail(user) {
  // One live token per user — a resend replaces the previous link rather
  // than leaving it valid alongside a new one.
  db.prepare(`DELETE FROM email_verifications WHERE user_id = ?`).run(user.id);
  const token = newToken();
  const expires = new Date(Date.now() + VERIFY_MS).toISOString();
  db.prepare(
    `INSERT INTO email_verifications (token_digest, user_id, expires_at) VALUES (?, ?, ?)`,
  ).run(digest(token), user.id, expires);
  const link = `https://dolphinclient.de/verify?token=${token}`;
  await sendMail(
    user.email,
    "Verify your DolphinClient email",
    `Welcome to DolphinClient! Confirm this is your email address: ${link}\n(valid for 24 hours)\n\nIf you didn't create this account, you can ignore this email.`,
  );
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
  let socialLinks = {};
  try {
    socialLinks = u.social_links ? JSON.parse(u.social_links) : {};
  } catch {
    socialLinks = {};
  }
  return {
    id: u.id,
    email: u.email,
    display_name: u.display_name,
    minecraft_username: u.minecraft_username,
    minecraft_uuid: u.minecraft_uuid,
    created_at: u.created_at,
    last_login_at: u.last_login_at,
    email_verified: Boolean(u.email_verified_at),
    bio: u.bio || "",
    avatar_url: u.avatar_url || "",
    social_links: socialLinks,
    totp_enabled: Boolean(u.totp_enabled_at),
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
            `INSERT INTO users (email, password_hash, display_name, created_at, email_verified_at)
             VALUES (?, ?, ?, ?, ?)`,
          )
          // No mail server configured means there's no way to prove the
          // address, so don't dangle an unusable "please verify" state —
          // treat it as verified, same as the forgot-password feature being
          // silently off in that case.
          .run(email, hashPassword(password), displayName, now(), mailConfigured() ? null : now());
      } catch (e) {
        if (e.code === "SQLITE_CONSTRAINT_UNIQUE" || e.code === "SQLITE_CONSTRAINT") {
          return json(res, 409, { error: "an account with that email already exists" });
        }
        throw e;
      }
      const token = createSession(info.lastInsertRowid, req);
      setSessionCookie(res, token);
      const user = db.prepare(`SELECT * FROM users WHERE id = ?`).get(info.lastInsertRowid);
      recordLogin(user.id, req, true, "register");
      if (mailConfigured()) await sendVerificationEmail(user);
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
        if (user) recordLogin(user.id, req, false, "wrong_password");
        return json(res, 401, { error: "wrong email or password" });
      }
      if (user.banned_at) {
        recordLogin(user.id, req, false, "banned");
        return json(res, 403, { error: "this account has been banned" });
      }
      if (user.totp_enabled_at) {
        // Password checks out, but a second factor is required: hand back a
        // short-lived challenge instead of a session cookie. No session
        // exists yet, so this can't be used for anything but /auth/login/totp.
        const challenge = newToken();
        const expires = new Date(Date.now() + 5 * 60_000).toISOString();
        db.prepare(`INSERT INTO totp_challenges (token_digest, user_id, expires_at) VALUES (?, ?, ?)`).run(
          digest(challenge),
          user.id,
          expires,
        );
        return json(res, 200, { requires_totp: true, challenge });
      }
      db.prepare(`UPDATE users SET last_login_at = ? WHERE id = ?`).run(now(), user.id);
      const token = createSession(user.id, req);
      setSessionCookie(res, token);
      recordLogin(user.id, req, true, "ok");
      return json(res, 200, { user: publicUser(user) });
    }

    // POST /auth/login/totp — complete a login that /auth/login flagged as
    // requiring 2FA. Accepts either a 6-digit app code or an unused backup
    // code, either way spends the one-time challenge from the first step.
    if (method === "POST" && path === "/auth/login/totp") {
      const ip = reqIp(req);
      if (rateLimited(`login-totp:${ip}`)) return json(res, 429, { error: "too many attempts, try later" });
      const body = await readJson(req);
      const challenge = str(body.challenge, 200);
      const row = challenge
        ? db
            .prepare(`SELECT * FROM totp_challenges WHERE token_digest = ? AND expires_at > ?`)
            .get(digest(challenge), now())
        : null;
      if (!row) return json(res, 400, { error: "that login has expired — sign in again" });
      const challengeUser = db.prepare(`SELECT * FROM users WHERE id = ?`).get(row.user_id);
      if (!challengeUser || challengeUser.banned_at) {
        db.prepare(`DELETE FROM totp_challenges WHERE token_digest = ?`).run(row.token_digest);
        return json(res, 403, { error: "this account can't sign in right now" });
      }
      const ok = verifyTotp(challengeUser.totp_secret, body.code) || consumeBackupCode(challengeUser, body.code);
      if (!ok) {
        recordLogin(challengeUser.id, req, false, "totp_failed");
        return json(res, 400, { error: "that code is wrong or expired" });
      }
      db.prepare(`DELETE FROM totp_challenges WHERE token_digest = ?`).run(row.token_digest);
      db.prepare(`UPDATE users SET last_login_at = ? WHERE id = ?`).run(now(), challengeUser.id);
      const token = createSession(challengeUser.id, req);
      setSessionCookie(res, token);
      recordLogin(challengeUser.id, req, true, "ok");
      const fresh = db.prepare(`SELECT * FROM users WHERE id = ?`).get(challengeUser.id);
      return json(res, 200, { user: publicUser(fresh) });
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

    // POST /auth/verify — confirm an email address from a link's token.
    if (method === "POST" && path === "/auth/verify") {
      const body = await readJson(req);
      const token = str(body.token, 200);
      if (!token) return json(res, 400, { error: "missing token" });
      const row = db
        .prepare(`SELECT * FROM email_verifications WHERE token_digest = ? AND expires_at > ?`)
        .get(digest(token), now());
      if (!row) return json(res, 400, { error: "that verification link is invalid or has expired" });
      db.prepare(`UPDATE users SET email_verified_at = ? WHERE id = ?`).run(now(), row.user_id);
      db.prepare(`DELETE FROM email_verifications WHERE user_id = ?`).run(row.user_id);
      return json(res, 200, { ok: true });
    }

    // Everything below requires a signed-in session.
    if (!user) return json(res, 401, { error: "not signed in" });

    // GET /me
    if (method === "GET" && path === "/me") {
      return json(res, 200, { user: publicUser(user) });
    }

    // POST /auth/resend-verification
    if (method === "POST" && path === "/auth/resend-verification") {
      if (user.email_verified_at) return json(res, 200, { ok: true, already_verified: true });
      if (!mailConfigured()) {
        return json(res, 503, { error: "email verification is not set up on this server" });
      }
      if (rateLimited(`verify:${user.id}`)) return json(res, 429, { error: "too many attempts, try later" });
      await sendVerificationEmail(user);
      return json(res, 200, { ok: true });
    }

    // GET /sessions — every active session for this account, oldest last.
    if (method === "GET" && path === "/sessions") {
      const rows = db
        .prepare(
          `SELECT token_digest, created_at, expires_at, ip, user_agent FROM sessions
           WHERE user_id = ? AND expires_at > ? ORDER BY created_at DESC`,
        )
        .all(user.id, now());
      const currentDigest = digest(sessionToken);
      return json(res, 200, {
        sessions: rows.map((r) => ({
          id: r.token_digest,
          created_at: r.created_at,
          expires_at: r.expires_at,
          ip: r.ip,
          user_agent: r.user_agent,
          current: r.token_digest === currentDigest,
        })),
      });
    }

    // DELETE /sessions/:id — end one other session (id is its token digest,
    // a one-way hash of the token — it identifies the session but, unlike
    // the token itself, can't be used to sign in as it).
    if (method === "DELETE" && path.startsWith("/sessions/")) {
      const id = path.slice("/sessions/".length);
      const row = db.prepare(`SELECT user_id FROM sessions WHERE token_digest = ?`).get(id);
      if (!row || row.user_id !== user.id) return json(res, 404, { error: "no such session" });
      db.prepare(`DELETE FROM sessions WHERE token_digest = ?`).run(id);
      return json(res, 200, { ok: true });
    }

    // GET /login-history — the last 50 login attempts for this account,
    // successful or not, newest first. Geolocation is filled in
    // best-effort and may be null even for a public IP.
    if (method === "GET" && path === "/login-history") {
      const rows = db
        .prepare(
          `SELECT created_at, ip, user_agent, success, reason, country, city
           FROM login_history WHERE user_id = ? ORDER BY created_at DESC LIMIT 50`,
        )
        .all(user.id);
      return json(res, 200, {
        history: rows.map((r) => ({
          created_at: r.created_at,
          ip: r.ip,
          user_agent: r.user_agent,
          success: Boolean(r.success),
          reason: r.reason,
          country: r.country,
          city: r.city,
        })),
      });
    }

    // POST /totp/setup — start enabling 2FA: generate a secret, park it
    // until confirmed with a real code (so a half-finished setup never
    // locks the account into a state it can't sign in to).
    if (method === "POST" && path === "/totp/setup") {
      if (user.totp_enabled_at) return json(res, 409, { error: "two-factor authentication is already enabled" });
      const secret = generateTotpSecret();
      db.prepare(
        `INSERT INTO totp_pending (user_id, secret, created_at) VALUES (?, ?, ?)
         ON CONFLICT(user_id) DO UPDATE SET secret = excluded.secret, created_at = excluded.created_at`,
      ).run(user.id, secret, now());
      return json(res, 200, { secret, otpauth_url: totpAuthUrl(user.email, secret) });
    }

    // POST /totp/confirm — finish enabling 2FA with a code from the app;
    // returns the one-time backup codes.
    if (method === "POST" && path === "/totp/confirm") {
      const pending = db.prepare(`SELECT secret FROM totp_pending WHERE user_id = ?`).get(user.id);
      if (!pending) return json(res, 400, { error: "no pending setup — call /totp/setup first" });
      const body = await readJson(req);
      if (!verifyTotp(pending.secret, body.code)) return json(res, 400, { error: "that code is wrong or expired" });
      const codes = generateBackupCodes();
      db.prepare(
        `UPDATE users SET totp_secret = ?, totp_enabled_at = ?, totp_backup_codes = ? WHERE id = ?`,
      ).run(pending.secret, now(), hashBackupCodes(codes), user.id);
      db.prepare(`DELETE FROM totp_pending WHERE user_id = ?`).run(user.id);
      return json(res, 200, { ok: true, backup_codes: codes });
    }

    // POST /totp/disable — requires the current password and a valid
    // code (app code or an unused backup code) so a hijacked session
    // alone can't turn 2FA off.
    if (method === "POST" && path === "/totp/disable") {
      if (!user.totp_enabled_at) return json(res, 409, { error: "two-factor authentication isn't enabled" });
      const body = await readJson(req);
      if (!verifyPassword(String(body.password ?? ""), user.password_hash)) {
        return json(res, 401, { error: "password is wrong" });
      }
      const ok = verifyTotp(user.totp_secret, body.code) || consumeBackupCode(user, body.code);
      if (!ok) return json(res, 400, { error: "that code is wrong or expired" });
      db.prepare(
        `UPDATE users SET totp_secret = NULL, totp_enabled_at = NULL, totp_backup_codes = NULL WHERE id = ?`,
      ).run(user.id);
      return json(res, 200, { ok: true });
    }

    // POST /totp/regenerate-backup-codes — invalidate the old set, issue a
    // fresh one. Requires the password since these codes bypass the app.
    if (method === "POST" && path === "/totp/regenerate-backup-codes") {
      if (!user.totp_enabled_at) return json(res, 409, { error: "two-factor authentication isn't enabled" });
      const body = await readJson(req);
      if (!verifyPassword(String(body.password ?? ""), user.password_hash)) {
        return json(res, 401, { error: "password is wrong" });
      }
      const codes = generateBackupCodes();
      db.prepare(`UPDATE users SET totp_backup_codes = ? WHERE id = ?`).run(hashBackupCodes(codes), user.id);
      return json(res, 200, { ok: true, backup_codes: codes });
    }

    // PATCH /profile — display name, bio, avatar, social links, and/or
    // Minecraft username.
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
      if (body.bio !== undefined) {
        sets.push("bio = ?");
        vals.push(cleanBio(body.bio));
      }
      if (body.avatar_url !== undefined) {
        const u = cleanAvatarUrl(body.avatar_url);
        if (u === null) return json(res, 400, { error: "avatar must be an http(s) URL" });
        sets.push("avatar_url = ?");
        vals.push(u);
      }
      if (body.social_links !== undefined) {
        const s = cleanSocialLinks(body.social_links);
        if (s === null) return json(res, 400, { error: "social handles: letters, digits, ., _, - only" });
        sets.push("social_links = ?");
        vals.push(s);
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
