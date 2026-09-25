#!/usr/bin/env node
// Builds the admin portal's data: reads the nginx access logs, the download
// manifest, the changelog, the account-api SQLite database (read-only) and a
// few system values, and writes all of it as ONE JSON file to
// /var/www/example.invalid/admin-data/stats.json.
//
// There is deliberately no backend: the admin page is static and loads only
// this file (behind Cloudflare Access + the origin gate, see ZERO-TRUST.md).
// Everything here is MEASURED — no estimated or invented numbers. Account
// figures are counts and dates only — no email address or password hash
// ever leaves account-api.mjs's own database.
//
// Aufruf (als root, z. B. per systemd-Timer alle 10 Minuten):
//   node deploy/admin-stats.mjs [webroot]
//
//   ACCOUNT_DB=/var/lib/dolphinclient/account-api/account.db   (optional,
//     same default as account-api.mjs — if the file doesn't exist,
//     `accounts` is simply null in the output)
import { createReadStream, existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync, statfsSync } from "node:fs";
import { createInterface } from "node:readline";
import { createGunzip } from "node:zlib";
import { execFileSync } from "node:child_process";
import { X509Certificate } from "node:crypto";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import Database from "better-sqlite3";

// Where this script lives — /opt/dolphinclient when installed by the setup
// script, the repo itself when run by hand. The git information below has to
// come from the REPO, so it is passed in (DOLPHIN_REPO) and only falls back to
// the script's own directory when that happens to be a checkout.
const ROOT = fileURLToPath(new URL("..", import.meta.url));
const REPO = process.env.DOLPHIN_REPO || ROOT;
const WEBROOT = process.argv[2] || "/var/www/example.invalid";
const LOGDIR = process.env.NGINX_LOG_DIR || "/var/log/nginx";
const OUT_DIR = join(WEBROOT, "admin-data");
const DAYS = 30;

// ---------------------------------------------------------------------------
// Logs
// ---------------------------------------------------------------------------
// nginx "combined": addr - user [time] "req" status bytes "ref" "ua"
const LINE =
  /^(\S+) \S+ \S+ \[([^\]]+)\] "([A-Z]+) ([^" ]*)[^"]*" (\d{3}) (\d+|-) "([^"]*)" "([^"]*)"/;

const MONTHS = { Jan: 0, Feb: 1, Mar: 2, Apr: 3, May: 4, Jun: 5, Jul: 6, Aug: 7, Sep: 8, Oct: 9, Nov: 10, Dec: 11 };

/** "12/Aug/2026:18:07:05 +0200" -> Date (or null). */
function parseTime(s) {
  const m = /^(\d{2})\/(\w{3})\/(\d{4}):(\d{2}):(\d{2}):(\d{2}) ([+-])(\d{2})(\d{2})$/.exec(s);
  if (!m) return null;
  const off = (m[7] === "-" ? -1 : 1) * (Number(m[8]) * 60 + Number(m[9]));
  return new Date(
    Date.UTC(+m[3], MONTHS[m[2]] ?? 0, +m[1], +m[4], +m[5], +m[6]) - off * 60_000,
  );
}

/** Paths that only this site serves — used to pick our own lines out of the
 *  shared log of the days before the site got its own log file. */
const OURS =
  /^\/(?:$|index|download|downloads|features|changelog|admin|_next|favicon|logo|404)/;

const isBot = (ua) => /bot|crawl|spider|slurp|scan|curl|wget|python-requests|libwww|headless/i.test(ua);

async function readLog(file, onLine) {
  const raw = createReadStream(file);
  const stream = file.endsWith(".gz") ? raw.pipe(createGunzip()) : raw;
  const rl = createInterface({ input: stream, crlfDelay: Infinity });
  for await (const line of rl) onLine(line);
}

const dayKey = (d) => d.toISOString().slice(0, 10);
const inc = (map, key, by = 1) => map.set(key, (map.get(key) || 0) + by);
const topN = (map, n, keyName = "name") =>
  [...map.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, n)
    .map(([k, v]) => ({ [keyName]: k, count: v }));

async function collect() {
  const since = new Date(Date.now() - DAYS * 86_400_000);
  const files = existsSync(LOGDIR)
    ? readdirSync(LOGDIR)
        .filter((f) => /(access|downloads)\.log(\.\d+)?(\.gz)?$/.test(f))
        .map((f) => join(LOGDIR, f))
    : [];

  const dedicated = files.filter((f) => f.includes("dolphinclient"));
  const shared = files.filter((f) => !f.includes("dolphinclient"));
  // The dedicated log only exists from the day it was switched on; before that
  // the site shared one log with the other vhosts on this host.
  let dedicatedFrom = null;

  const stats = {
    requests: 0,
    lines: 0,
    pageviews: new Map(), // day -> count
    visitors: new Map(), // day -> Set(ip)
    pages: new Map(),
    referrers: new Map(),
    agents: new Map(),
    countriesUnknown: 0,
    errors: new Map(), // "404 /path" -> count
    dlByAsset: new Map(),
    dlByVersion: new Map(),
    dlBytes: 0,
    dlDaily: new Map(),
    dlTotal: 0,
    updateChecks: 0,
    updateDaily: new Map(),
    botHits: 0,
    lastRequest: null,
  };

  const handle = (line, isDedicated) => {
    stats.lines++;
    const m = LINE.exec(line);
    if (!m) return;
    const [, ip, timeStr, method, rawPath, statusStr, bytesStr, referrer, ua] = m;
    // Next's client-side router prefetches `<route>/index.txt?_rsc=…` — the
    // same page, not a second visit. Judge everything by the bare path.
    const path = rawPath.split("?")[0];
    const time = parseTime(timeStr);
    if (!time || time < since) return;
    if (!isDedicated && !OURS.test(path)) return; // another vhost on this host
    if (isDedicated && (!dedicatedFrom || time < dedicatedFrom)) dedicatedFrom = time;

    stats.requests++;
    const day = dayKey(time);
    const status = Number(statusStr);
    const bytes = bytesStr === "-" ? 0 : Number(bytesStr);
    if (!stats.lastRequest || time > stats.lastRequest) stats.lastRequest = time;

    if (isBot(ua)) stats.botHits++;

    // --- downloads -------------------------------------------------------
    if (path.startsWith("/downloads/") && /\.(exe|zip|jar|dmg|appimage)$/i.test(path)) {
      // Only real transfers count (200 full, 206 range-resume).
      if (status === 200 || status === 206) {
        const name = path.slice(path.lastIndexOf("/") + 1);
        inc(stats.dlByAsset, name);
        inc(stats.dlDaily, day);
        stats.dlTotal++;
        stats.dlBytes += bytes;
        const ver = /\/downloads\/client\/([^/]+)\//.exec(path);
        if (ver) inc(stats.dlByVersion, ver[1]);
      }
    } else if (path === "/downloads/manifest.json") {
      stats.updateChecks++;
      inc(stats.updateDaily, day);
    } else if (method === "GET" && status < 400 && !/\.[a-z0-9]{2,5}$/i.test(path)) {
      // A page view: a route, not an asset.
      if (!isBot(ua)) {
        inc(stats.pageviews, day);
        inc(stats.pages, path);
        if (!stats.visitors.has(day)) stats.visitors.set(day, new Set());
        stats.visitors.get(day).add(ip);
        if (referrer && referrer !== "-" && !referrer.includes("example.invalid")) {
          inc(stats.referrers, referrer.slice(0, 120));
        }
        inc(stats.agents, shortAgent(ua));
      }
    }

    if (status >= 400 && status !== 499) inc(stats.errors, `${status} ${path.slice(0, 80)}`);
  };

  for (const f of dedicated) await readLog(f, (l) => handle(l, true));
  for (const f of shared) await readLog(f, (l) => handle(l, false));

  const days = [];
  for (let i = DAYS - 1; i >= 0; i--) {
    const d = dayKey(new Date(Date.now() - i * 86_400_000));
    days.push({
      day: d,
      views: stats.pageviews.get(d) || 0,
      visitors: stats.visitors.get(d)?.size || 0,
      downloads: stats.dlDaily.get(d) || 0,
      updateChecks: stats.updateDaily.get(d) || 0,
    });
  }

  return {
    windowDays: DAYS,
    logFiles: files.length,
    logLines: stats.lines,
    requests: stats.requests,
    lastRequest: stats.lastRequest?.toISOString() || null,
    dedicatedLogFrom: dedicatedFrom?.toISOString() || null,
    botHits: stats.botHits,
    days,
    downloads: {
      total: stats.dlTotal,
      bytes: stats.dlBytes,
      byAsset: topN(stats.dlByAsset, 12, "asset"),
      byVersion: topN(stats.dlByVersion, 12, "version"),
    },
    updateChecks: stats.updateChecks,
    topPages: topN(stats.pages, 10, "path"),
    topReferrers: topN(stats.referrers, 8, "referrer"),
    topAgents: topN(stats.agents, 8, "agent"),
    errors: topN(stats.errors, 10, "what"),
  };
}

/** Collapse a user agent into something readable. */
function shortAgent(ua) {
  if (/DolphinClient/i.test(ua)) return "DolphinClient launcher";
  if (isBot(ua)) return "Bot / crawler";
  const os = /Windows/i.test(ua) ? "Windows" : /Android/i.test(ua) ? "Android" : /iPhone|iPad/i.test(ua) ? "iOS" : /Mac OS X/i.test(ua) ? "macOS" : /Linux/i.test(ua) ? "Linux" : "Other";
  const br = /Edg\//.test(ua) ? "Edge" : /OPR\//.test(ua) ? "Opera" : /Chrome\//.test(ua) ? "Chrome" : /Firefox\//.test(ua) ? "Firefox" : /Safari\//.test(ua) ? "Safari" : "Other";
  return `${br} · ${os}`;
}

// ---------------------------------------------------------------------------
// Release + System
// ---------------------------------------------------------------------------
function readJson(file) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch {
    return null;
  }
}

function sh(cmd, args) {
  try {
    return execFileSync(cmd, args, { encoding: "utf8", timeout: 10_000 }).trim();
  } catch {
    return null;
  }
}

function dirBytes(dir) {
  const out = sh("du", ["-sb", dir]);
  return out ? Number(out.split(/\s+/)[0]) : null;
}

function certInfo() {
  const dir = "/etc/letsencrypt/live";
  if (!existsSync(dir)) return null;
  for (const name of readdirSync(dir)) {
    const pem = join(dir, name, "fullchain.pem");
    if (!existsSync(pem)) continue;
    try {
      const c = new X509Certificate(readFileSync(pem));
      return { name, subject: c.subject.replace(/^CN=/, ""), validTo: new Date(c.validTo).toISOString() };
    } catch {
      /* next */
    }
  }
  return null;
}

function releaseInfo() {
  const manifest = readJson(join(WEBROOT, "downloads", "manifest.json"));
  if (!manifest) return null;
  // Only what is CURRENT: the per-OS installer and the client binary the
  // launcher downloads. `clientVersions` is the archive — counted, not listed.
  const assets = [];
  const push = (what, o) => {
    if (!o || typeof o.size !== "number") return;
    assets.push({
      what,
      name: o.file || o.url || what,
      size: o.size,
      sha256: (o.sha256 || "").slice(0, 12),
      url: o.url || null,
    });
  };
  for (const [os, p] of Object.entries(manifest.platforms || {})) {
    if (p?.available) push(`Installer · ${p.label || os}`, p);
  }
  for (const [os, c] of Object.entries(manifest.client || {})) {
    if (os !== "version") push(`Client · ${os}`, c);
  }
  return {
    version: manifest.version || null,
    minecraft: manifest.minecraft || null,
    generatedAt: manifest.generatedAt || null,
    archived: Array.isArray(manifest.clientVersions) ? manifest.clientVersions.length : 0,
    platforms: Object.entries(manifest.platforms || {}).map(([os, p]) => ({
      os,
      label: p?.label || os,
      available: !!p?.available,
    })),
    assets,
  };
}

function releaseHistory() {
  const changes = readJson(join(WEBROOT, "downloads", "changelog.json")) || [];
  const clientDir = join(WEBROOT, "downloads", "client");
  const published = new Map();
  if (existsSync(clientDir)) {
    for (const v of readdirSync(clientDir)) {
      try {
        published.set(`v${v}`, statSync(join(clientDir, v)).mtime.toISOString());
      } catch {
        /* skip */
      }
    }
  }
  return changes.slice(0, 14).map((c) => ({
    v: c.v,
    headline: String(c.date || "").replace(/^(Current|Aktuell)\s*·\s*/, ""),
    current: /^(Current|Aktuell)\s*·/.test(String(c.date || "")),
    items: Array.isArray(c.items) ? c.items.length : 0,
    shots: Array.isArray(c.shots) ? c.shots.length : 0,
    published: published.get(c.v) || null,
  }));
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------
// Reads account-api.mjs's own SQLite database directly, read-only — no HTTP
// call to that service, matching this script's "no backend" model for the
// admin page. Same default path/env var as account-api.mjs itself.
function accountStats() {
  const dbPath = process.env.ACCOUNT_DB || "/var/lib/dolphinclient/account-api/account.db";
  if (!existsSync(dbPath)) return null;
  let db;
  try {
    db = new Database(dbPath, { readonly: true, fileMustExist: true });
    const count = (sql, ...args) => db.prepare(sql).get(...args).n;
    const total = count(`SELECT COUNT(*) AS n FROM users`);
    const verified = count(`SELECT COUNT(*) AS n FROM users WHERE email_verified_at IS NOT NULL`);
    const withMinecraft = count(`SELECT COUNT(*) AS n FROM users WHERE minecraft_uuid IS NOT NULL`);
    const activeSessions = count(
      `SELECT COUNT(*) AS n FROM sessions WHERE expires_at > ?`,
      new Date().toISOString(),
    );
    const since = new Date(Date.now() - DAYS * 86_400_000).toISOString();
    const signupsByDay = new Map(
      db
        .prepare(
          `SELECT substr(created_at, 1, 10) AS day, COUNT(*) AS n FROM users
           WHERE created_at >= ? GROUP BY day`,
        )
        .all(since)
        .map((r) => [r.day, r.n]),
    );
    return { total, verified, withMinecraft, activeSessions, signupsByDay };
  } catch (e) {
    console.error(`admin-stats: couldn't read the account database: ${e.message}`);
    return null;
  } finally {
    db?.close();
  }
}

function systemInfo() {
  let disk = null;
  try {
    const s = statfsSync(WEBROOT);
    disk = { free: s.bfree * s.bsize, total: s.blocks * s.bsize };
  } catch {
    /* no statfs */
  }
  const load = sh("cat", ["/proc/loadavg"]);
  const uptime = sh("cat", ["/proc/uptime"]);
  return {
    nginx: sh("systemctl", ["is-active", "nginx"]) || "unknown",
    accessGate: sh("systemctl", ["is-active", "dolphinclient-access"]) || "not installed",
    disk,
    webrootBytes: dirBytes(WEBROOT),
    downloadsBytes: dirBytes(join(WEBROOT, "downloads")),
    load: load ? load.split(" ").slice(0, 3).map(Number) : null,
    uptimeSeconds: uptime ? Math.round(Number(uptime.split(" ")[0])) : null,
    cert: certInfo(),
    hostname: sh("hostname", []) || null,
    // The timer runs as root while the checkout belongs to another user, and
    // git refuses that ("dubious ownership") unless it is told this is fine.
    git: (() => {
      const g = (...args) => sh("git", ["-c", `safe.directory=${REPO}`, "-C", REPO, ...args]);
      return {
        head: g("rev-parse", "--short", "HEAD"),
        subject: g("log", "-1", "--pretty=%s"),
        when: g("log", "-1", "--pretty=%cI"),
        dirty: (g("status", "--porcelain") || "") !== "",
      };
    })(),
    deployedAt: existsSync(join(WEBROOT, "index.html"))
      ? statSync(join(WEBROOT, "index.html")).mtime.toISOString()
      : null,
  };
}

// ---------------------------------------------------------------------------
const traffic = await collect();
const accounts = accountStats();
// Fold signups into the same per-day series the traffic charts already use,
// so the admin page can chart them with the exact same Bars component.
for (const d of traffic.days) d.signups = accounts?.signupsByDay.get(d.day) || 0;

const out = {
  generated: new Date().toISOString(),
  webroot: WEBROOT,
  release: releaseInfo(),
  history: releaseHistory(),
  traffic,
  accounts: accounts
    ? {
        total: accounts.total,
        verified: accounts.verified,
        withMinecraft: accounts.withMinecraft,
        activeSessions: accounts.activeSessions,
      }
    : null,
  system: systemInfo(),
};

mkdirSync(OUT_DIR, { recursive: true });
const file = join(OUT_DIR, "stats.json");
writeFileSync(file, JSON.stringify(out, null, 1) + "\n");
try {
  execFileSync("chown", ["-R", "www-data:www-data", OUT_DIR]);
} catch {
  /* not root — fine for a dry run */
}
console.log(
  `admin-stats: ${file} — ${traffic.requests} Anfragen aus ${traffic.logFiles} Logdatei(en), ` +
    `${traffic.downloads.total} Downloads, ${traffic.updateChecks} Update-Checks` +
    (accounts ? `, ${accounts.total} Konten (${accounts.verified} verifiziert).` : ", keine Konto-DB gefunden."),
);
