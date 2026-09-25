#!/usr/bin/env node
// The admin portal's write API: publish, edit, reorder and delete changelog
// entries, upload or remove the screenshots that go with them, and manage
// accounts (search, ban/unban, delete).
//
// It writes exactly three things:
//   downloads/changelog.json      the version history the website fetches
//   downloads/shots/<v>/<file>    that release's screenshots
//   account-api's SQLite database (users table for bans/deletes)
// Changelog writes are runtime-published so edits are live the moment saved.
// Account writes are directly in the database.
// Every write of changelog keeps a timestamped copy in
// /var/lib/dolphinclient/changelog-history/ (outside the web root), so a bad
// edit is one restore away.
//
// Every request is verified with the SAME Cloudflare Access check as the rest
// of the portal (access-verify.mjs), on top of nginx's `auth_request` and the
// Cloudflare-origin check. Unconfigured ⇒ 503; no/!valid token ⇒ 401. It binds
// 127.0.0.1 only and is never reachable except through that gate.
//
//   ADMIN_API_LISTEN=127.0.0.1:8788        (optional)
//   DOLPHIN_WEBROOT=/var/www/dolphinclient.de
//   DOLPHIN_HISTORY=/var/lib/dolphinclient/changelog-history
//   ACCOUNT_DB=/var/lib/dolphinclient/account-api/account.db  (optional)
import { createServer } from "node:http";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import Database from "better-sqlite3";
import { requireIdentity, configured } from "./access-verify.mjs";

const [HOST, PORT] = (process.env.ADMIN_API_LISTEN || "127.0.0.1:8788").split(":");
const WEBROOT = process.env.DOLPHIN_WEBROOT || "/var/www/dolphinclient.de";
const DOWNLOADS = join(WEBROOT, "downloads");
const CHANGELOG = join(DOWNLOADS, "changelog.json");
const SHOTS = join(DOWNLOADS, "shots");
const HISTORY = process.env.DOLPHIN_HISTORY || "/var/lib/dolphinclient/changelog-history";
const ACCOUNT_DB = process.env.ACCOUNT_DB || "/var/lib/dolphinclient/account-api/account.db";

// Account database is optional (a server that hasn't set up account-api yet
// simply won't have account management endpoints available).
let accountDb = null;
try {
  if (existsSync(ACCOUNT_DB)) {
    accountDb = new Database(ACCOUNT_DB, { readonly: false });
  }
} catch {
  console.error(`admin-api: couldn't open account database at ${ACCOUNT_DB}`);
}

const MAX_JSON = 2 * 1024 * 1024; // a whole history is ~200 kB today
const MAX_IMAGE = 8 * 1024 * 1024;
const KEEP_BACKUPS = 40;

// --- shapes ----------------------------------------------------------------
// Everything written here ends up on a public page, and the file names become
// paths on disk. Both are validated, not trusted.

/** `0.61.0` / `v0.61.0` -> `v0.61.0`; anything else -> null. */
function cleanVersion(v) {
  const m = /^v?(\d{1,3})\.(\d{1,3})\.(\d{1,4})$/.exec(String(v || "").trim());
  return m ? `v${m[1]}.${m[2]}.${m[3]}` : null;
}

/** A screenshot file name — no slashes, no dots-dots, known image suffix. */
function cleanFile(name) {
  const n = String(name || "").trim();
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,80}$/.test(n)) return null;
  if (n.includes("..")) return null;
  return /\.(png|jpe?g|webp|gif)$/i.test(n) ? n : null;
}

const str = (s, max) => String(s ?? "").replace(/\r/g, "").slice(0, max);

/** Validate one entry. Returns {entry} or {error}. */
function cleanEntry(raw) {
  if (!raw || typeof raw !== "object") return { error: "entry must be an object" };
  const v = cleanVersion(raw.v);
  if (!v) return { error: `bad version: ${JSON.stringify(raw.v)} (expected 0.61.0)` };
  const date = str(raw.date, 120).trim();
  if (!date) return { error: `${v}: the headline/date line must not be empty` };

  const items = Array.isArray(raw.items) ? raw.items : [];
  if (!items.length) return { error: `${v}: at least one bullet is required` };
  if (items.length > 200) return { error: `${v}: too many bullets (max 200)` };
  const cleanItems = [];
  for (const it of items) {
    const t = str(it, 4000).trim();
    if (t) cleanItems.push(t);
  }
  if (!cleanItems.length) return { error: `${v}: all bullets are empty` };

  const shots = [];
  for (const s of Array.isArray(raw.shots) ? raw.shots : []) {
    const src = cleanFile(s?.src);
    if (!src) return { error: `${v}: bad screenshot name ${JSON.stringify(s?.src)}` };
    shots.push({ src, alt: str(s?.alt, 300).trim() || src });
    if (shots.length > 40) return { error: `${v}: too many screenshots (max 40)` };
  }

  const entry = { v, date, items: cleanItems };
  if (shots.length) entry.shots = shots;
  return { entry };
}

/** Validate a whole history: every entry valid, no duplicate versions. */
function cleanList(raw) {
  if (!Array.isArray(raw)) return { error: "expected a list of entries" };
  if (!raw.length) return { error: "the history must not be empty" };
  if (raw.length > 500) return { error: "too many entries" };
  const out = [];
  const seen = new Set();
  for (const r of raw) {
    const { entry, error } = cleanEntry(r);
    if (error) return { error };
    if (seen.has(entry.v)) return { error: `${entry.v} appears twice` };
    seen.add(entry.v);
    out.push(entry);
  }
  return { list: out };
}

// --- storage ---------------------------------------------------------------
function readList() {
  try {
    const parsed = JSON.parse(readFileSync(CHANGELOG, "utf8"));
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function backup(reason, email) {
  if (!existsSync(CHANGELOG)) return null;
  mkdirSync(HISTORY, { recursive: true });
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  const file = join(HISTORY, `changelog-${stamp}.json`);
  writeFileSync(file, readFileSync(CHANGELOG));
  writeFileSync(
    `${file}.meta`,
    JSON.stringify({ at: new Date().toISOString(), reason, by: email }),
  );
  // Keep the last N; a changelog is small but this runs unattended.
  const old = readdirSync(HISTORY)
    .filter((f) => f.endsWith(".json"))
    .sort()
    .slice(0, -KEEP_BACKUPS);
  for (const f of old) {
    rmSync(join(HISTORY, f), { force: true });
    rmSync(join(HISTORY, `${f}.meta`), { force: true });
  }
  return file;
}

/** Write the history atomically (tmp + rename) so a reader never sees half. */
function writeList(list, reason, email) {
  mkdirSync(DOWNLOADS, { recursive: true });
  backup(reason, email);
  const tmp = `${CHANGELOG}.tmp-${process.pid}`;
  writeFileSync(tmp, `${JSON.stringify(list, null, 2)}\n`);
  renameSync(tmp, CHANGELOG);
  console.log(`admin-api: ${reason} by ${email} — ${list.length} entries`);
}

function shotFiles(version) {
  const dir = join(SHOTS, version);
  try {
    return readdirSync(dir)
      .filter((f) => cleanFile(f))
      .sort()
      .map((f) => {
        const st = statSync(join(dir, f));
        return { src: f, bytes: st.size, at: st.mtime.toISOString() };
      });
  } catch {
    return [];
  }
}

// --- HTTP ------------------------------------------------------------------
const json = (res, code, body) =>
  res
    .writeHead(code, { "content-type": "application/json", "cache-control": "no-store" })
    .end(JSON.stringify(body));

function readBody(req, limit) {
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

/** Recognise the bytes, not the claimed content type. */
function imageKind(buf) {
  if (buf.length < 12) return null;
  if (buf.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])))
    return "png";
  if (buf[0] === 0xff && buf[1] === 0xd8 && buf[2] === 0xff) return "jpeg";
  if (buf.subarray(0, 4).toString() === "RIFF" && buf.subarray(8, 12).toString() === "WEBP")
    return "webp";
  if (buf.subarray(0, 6).toString() === "GIF89a" || buf.subarray(0, 6).toString() === "GIF87a")
    return "gif";
  return null;
}

const server = createServer(async (req, res) => {
  if (req.url === "/health") {
    return json(res, 200, {
      configured: configured(),
      entries: readList().length,
      webroot: WEBROOT,
    });
  }

  const who = await requireIdentity(req, res);
  if (!who) return; // already answered 401/503

  const url = new URL(req.url, "http://api");
  const path = url.pathname.replace(/\/+$/, "") || "/";
  const seg = path.split("/").filter(Boolean);
  const method = req.method || "GET";

  try {
    // GET /changelog — the live history, exactly as the website sees it.
    if (method === "GET" && path === "/changelog") {
      return json(res, 200, { entries: readList(), by: who.email });
    }

    // PUT /changelog — replace the whole history (used for reordering).
    if (method === "PUT" && path === "/changelog") {
      const body = JSON.parse((await readBody(req, MAX_JSON)).toString("utf8"));
      const { list, error } = cleanList(body.entries ?? body);
      if (error) return json(res, 400, { error });
      writeList(list, "replace history", who.email);
      return json(res, 200, { entries: list });
    }

    // POST /changelog — add a new entry at the top.
    if (method === "POST" && path === "/changelog") {
      const body = JSON.parse((await readBody(req, MAX_JSON)).toString("utf8"));
      const { entry, error } = cleanEntry(body);
      if (error) return json(res, 400, { error });
      const list = readList();
      if (list.some((e) => e.v === entry.v)) {
        return json(res, 409, { error: `${entry.v} already exists — edit it instead` });
      }
      // Vanilla house style: only the newest entry carries the "Current · "
      // prefix, so publishing a release demotes the one before it.
      const next = list.map((e) => ({ ...e, date: String(e.date).replace(/^Current · /, "") }));
      if (!entry.date.startsWith("Current · ")) entry.date = `Current · ${entry.date}`;
      next.unshift(entry);
      writeList(next, `publish ${entry.v}`, who.email);
      return json(res, 201, { entry, entries: next });
    }

    // PUT /changelog/<v> — edit one entry in place.
    if (method === "PUT" && seg[0] === "changelog" && seg.length === 2) {
      const v = cleanVersion(seg[1]);
      if (!v) return json(res, 400, { error: "bad version in the path" });
      const body = JSON.parse((await readBody(req, MAX_JSON)).toString("utf8"));
      const { entry, error } = cleanEntry({ ...body, v: body.v ?? v });
      if (error) return json(res, 400, { error });
      const list = readList();
      const at = list.findIndex((e) => cleanVersion(e.v) === v);
      if (at === -1) return json(res, 404, { error: `${v} not found` });
      if (entry.v !== v && list.some((e) => cleanVersion(e.v) === entry.v)) {
        return json(res, 409, { error: `${entry.v} already exists` });
      }
      list[at] = entry;
      writeList(list, `edit ${v}`, who.email);
      return json(res, 200, { entry, entries: list });
    }

    // DELETE /changelog/<v> — remove an entry (its screenshots stay on disk).
    if (method === "DELETE" && seg[0] === "changelog" && seg.length === 2) {
      const v = cleanVersion(seg[1]);
      if (!v) return json(res, 400, { error: "bad version in the path" });
      const list = readList();
      const next = list.filter((e) => cleanVersion(e.v) !== v);
      if (next.length === list.length) return json(res, 404, { error: `${v} not found` });
      if (!next.length) return json(res, 400, { error: "refusing to empty the history" });
      // Whatever was top stays top: re-mark the current entry.
      next[0] = {
        ...next[0],
        date: next[0].date.startsWith("Current · ")
          ? next[0].date
          : `Current · ${String(next[0].date).replace(/^Current · /, "")}`,
      };
      writeList(next, `delete ${v}`, who.email);
      return json(res, 200, { entries: next });
    }

    // GET /shots/<v> — what is actually on disk for that release.
    if (method === "GET" && seg[0] === "shots" && seg.length === 2) {
      const v = cleanVersion(seg[1]);
      if (!v) return json(res, 400, { error: "bad version" });
      return json(res, 200, { version: v, files: shotFiles(v) });
    }

    // POST /shots/<v>/<file> — upload one screenshot (raw image body).
    if (method === "POST" && seg[0] === "shots" && seg.length === 3) {
      const v = cleanVersion(seg[1]);
      const name = cleanFile(seg[2]);
      if (!v || !name) return json(res, 400, { error: "bad version or file name" });
      const buf = await readBody(req, MAX_IMAGE);
      const kind = imageKind(buf);
      if (!kind) return json(res, 400, { error: "that is not a PNG, JPEG, WebP or GIF" });
      const dir = join(SHOTS, v);
      mkdirSync(dir, { recursive: true });
      const tmp = join(dir, `.tmp-${process.pid}-${name}`);
      writeFileSync(tmp, buf);
      renameSync(tmp, join(dir, name));
      console.log(`admin-api: upload ${v}/${name} (${kind}, ${buf.length} B) by ${who.email}`);
      return json(res, 201, { version: v, src: name, bytes: buf.length, files: shotFiles(v) });
    }

    // DELETE /shots/<v>/<file> — remove a screenshot from disk and from every
    // entry that referenced it, so the page never points at a missing file.
    if (method === "DELETE" && seg[0] === "shots" && seg.length === 3) {
      const v = cleanVersion(seg[1]);
      const name = cleanFile(seg[2]);
      if (!v || !name) return json(res, 400, { error: "bad version or file name" });
      rmSync(join(SHOTS, v, name), { force: true });
      const list = readList();
      let touched = false;
      for (const e of list) {
        if (cleanVersion(e.v) !== v || !Array.isArray(e.shots)) continue;
        const kept = e.shots.filter((s) => s.src !== name);
        if (kept.length !== e.shots.length) {
          touched = true;
          if (kept.length) e.shots = kept;
          else delete e.shots;
        }
      }
      if (touched) writeList(list, `remove shot ${v}/${name}`, who.email);
      return json(res, 200, { version: v, files: shotFiles(v), entries: list });
    }

    // GET /history — the backups taken before each write.
    if (method === "GET" && path === "/history") {
      let files = [];
      try {
        files = readdirSync(HISTORY).filter((f) => f.endsWith(".json"));
      } catch {
        /* no backups yet */
      }
      const out = files
        .sort()
        .reverse()
        .map((f) => {
          let meta = {};
          try {
            meta = JSON.parse(readFileSync(join(HISTORY, `${f}.meta`), "utf8"));
          } catch {
            /* older backup without meta */
          }
          return { file: f, bytes: statSync(join(HISTORY, f)).size, ...meta };
        });
      return json(res, 200, { backups: out });
    }

    // POST /restore/<file> — put a backup back (itself backed up first).
    if (method === "POST" && seg[0] === "restore" && seg.length === 2) {
      const f = seg[1];
      if (!/^changelog-[0-9TZ.-]+\.json$/.test(f)) return json(res, 400, { error: "bad backup" });
      const src = join(HISTORY, f);
      if (!existsSync(src)) return json(res, 404, { error: "no such backup" });
      const { list, error } = cleanList(JSON.parse(readFileSync(src, "utf8")));
      if (error) return json(res, 400, { error: `backup is unusable: ${error}` });
      writeList(list, `restore ${f}`, who.email);
      return json(res, 200, { entries: list });
    }

    // --- account management (admin only) ---
    // GET /accounts — list accounts (with optional search).
    if (method === "GET" && path === "/accounts") {
      if (!accountDb) return json(res, 503, { error: "account database not configured" });
      const q = String(url.searchParams.get("q") || "").trim().toLowerCase();
      const limit = Math.min(Math.max(Number(url.searchParams.get("limit")) || 50, 1), 200);
      const offset = Math.max(Number(url.searchParams.get("offset")) || 0, 0);
      let query = `SELECT id, email, display_name, minecraft_username, created_at, last_login_at, banned_at
                   FROM users`;
      const params = [];
      if (q) {
        query += ` WHERE email LIKE ? OR display_name LIKE ?`;
        params.push(`%${q}%`, `%${q}%`);
      }
      query += ` ORDER BY created_at DESC LIMIT ? OFFSET ?`;
      params.push(limit + 1, offset); // fetch one extra to know if there are more
      const rows = accountDb.prepare(query).all(...params);
      const hasMore = rows.length > limit;
      if (hasMore) rows.pop(); // remove the extra row
      return json(res, 200, {
        accounts: rows.map((r) => ({
          id: r.id,
          email: r.email,
          display_name: r.display_name,
          minecraft_username: r.minecraft_username,
          created_at: r.created_at,
          last_login_at: r.last_login_at,
          banned: Boolean(r.banned_at),
        })),
        hasMore,
        offset,
        by: who.email,
      });
    }

    // PATCH /accounts/<id> — ban or unban an account.
    if (method === "PATCH" && seg[0] === "accounts" && seg.length === 2) {
      if (!accountDb) return json(res, 503, { error: "account database not configured" });
      const id = Number(seg[1]);
      if (!id || id < 1) return json(res, 400, { error: "bad account id" });
      const body = JSON.parse((await readBody(req, 1024)).toString("utf8"));
      const banned = Boolean(body.banned);
      const user = accountDb.prepare(`SELECT id FROM users WHERE id = ?`).get(id);
      if (!user) return json(res, 404, { error: "no such account" });
      const now = new Date().toISOString();
      if (banned) {
        accountDb.prepare(`UPDATE users SET banned_at = ? WHERE id = ?`).run(now, id);
      } else {
        accountDb.prepare(`UPDATE users SET banned_at = NULL WHERE id = ?`).run(id);
      }
      return json(res, 200, { id, banned, by: who.email });
    }

    // DELETE /accounts/<id> — delete an account.
    if (method === "DELETE" && seg[0] === "accounts" && seg.length === 2) {
      if (!accountDb) return json(res, 503, { error: "account database not configured" });
      const id = Number(seg[1]);
      if (!id || id < 1) return json(res, 400, { error: "bad account id" });
      const user = accountDb.prepare(`SELECT email FROM users WHERE id = ?`).get(id);
      if (!user) return json(res, 404, { error: "no such account" });
      accountDb.prepare(`DELETE FROM users WHERE id = ?`).run(id); // cascades sessions/resets
      return json(res, 200, { id, deleted_email: user.email, by: who.email });
    }

    return json(res, 404, { error: `no route for ${method} ${path}` });
  } catch (e) {
    const msg = String(e?.message || e);
    const code = msg === "too large" ? 413 : msg.includes("JSON") ? 400 : 500;
    console.error(`admin-api: ${method} ${path} failed: ${msg}`);
    return json(res, code, { error: msg });
  }
});

server.listen(Number(PORT), HOST, () => {
  console.log(
    `admin-api: http://${HOST}:${PORT} — ${configured() ? "ready" : "NOT CONFIGURED (everything 503)"}, webroot ${WEBROOT}`,
  );
});
