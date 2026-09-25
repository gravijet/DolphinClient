#!/usr/bin/env node
// Self-test for deploy/admin-stats.mjs's account-database reading.
//
// admin-stats.mjs is a one-shot script, not a server, so this runs the real
// thing as a subprocess against a throwaway webroot, log directory and
// account-api SQLite file, then checks the JSON it wrote. The nginx-log
// parsing has no test here — it needs no database and is exercised by hand
// against real logs; this covers the part that's easy to get wrong
// silently: reading someone else's database schema and getting the counts
// right.
//
//   node deploy/test-admin-stats.mjs
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import Database from "better-sqlite3";

const SCRIPT = fileURLToPath(new URL("./admin-stats.mjs", import.meta.url));

let failed = 0;
function check(name, ok, detail = "") {
  if (!ok) failed++;
  console.log(`${ok ? "  ok  " : "FAIL  "}${name}${detail ? `: ${detail}` : ""}`);
}

function runStats({ withDb }) {
  const webroot = mkdtempSync(join(tmpdir(), "dolphin-stats-test-"));
  mkdirSync(join(webroot, "downloads"), { recursive: true });
  writeFileSync(
    join(webroot, "downloads", "manifest.json"),
    JSON.stringify({ version: "0.0.0", platforms: {}, client: {} }),
  );
  writeFileSync(join(webroot, "downloads", "changelog.json"), "[]");
  const logDir = mkdtempSync(join(tmpdir(), "dolphin-stats-test-logs-"));

  const env = { ...process.env, NGINX_LOG_DIR: logDir };

  if (withDb) {
    const dbPath = join(mkdtempSync(join(tmpdir(), "dolphin-stats-test-db-")), "account.db");
    const db = new Database(dbPath);
    db.exec(`
      CREATE TABLE users (
        id INTEGER PRIMARY KEY, email TEXT, password_hash TEXT, display_name TEXT,
        minecraft_username TEXT, minecraft_uuid TEXT, created_at TEXT, last_login_at TEXT,
        email_verified_at TEXT
      );
      CREATE TABLE sessions (
        token_digest TEXT PRIMARY KEY, user_id INTEGER, created_at TEXT, expires_at TEXT,
        ip TEXT, user_agent TEXT
      );
    `);
    const today = new Date().toISOString();
    const anHourAgo = new Date(Date.now() - 3600_000).toISOString();
    const anHourFromNow = new Date(Date.now() + 3600_000).toISOString();
    db.prepare(
      `INSERT INTO users (email, password_hash, display_name, created_at, email_verified_at, minecraft_uuid)
       VALUES (?, ?, ?, ?, ?, ?)`,
    ).run("verified@example.com", "h", "Verified", today, today, "some-uuid");
    db.prepare(
      `INSERT INTO users (email, password_hash, display_name, created_at, email_verified_at)
       VALUES (?, ?, ?, ?, ?)`,
    ).run("unverified@example.com", "h", "Unverified", today, null);
    // One active session, one already expired — only the active one should count.
    db.prepare(`INSERT INTO sessions (token_digest, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)`).run(
      "active",
      1,
      anHourAgo,
      anHourFromNow,
    );
    db.prepare(`INSERT INTO sessions (token_digest, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)`).run(
      "expired",
      2,
      anHourAgo,
      anHourAgo,
    );
    db.close();
    env.ACCOUNT_DB = dbPath;
  } else {
    // Point at a file that doesn't exist — the script must degrade to
    // `accounts: null`, not throw.
    env.ACCOUNT_DB = join(mkdtempSync(join(tmpdir(), "dolphin-stats-test-nodb-")), "account.db");
  }

  execFileSync(process.execPath, [SCRIPT, webroot], { env, stdio: ["ignore", "ignore", "ignore"] });
  return JSON.parse(readFileSync(join(webroot, "admin-data", "stats.json"), "utf8"));
}

// --- with a real account database -------------------------------------------
{
  const stats = runStats({ withDb: true });
  check("counts every account", stats.accounts?.total === 2);
  check("counts only the verified one", stats.accounts?.verified === 1);
  check("counts only the linked-Minecraft one", stats.accounts?.withMinecraft === 1);
  check("counts only the still-active session", stats.accounts?.activeSessions === 1);
  const today = new Date().toISOString().slice(0, 10);
  const todayRow = stats.traffic.days.find((d) => d.day === today);
  check("folds today's two signups into the traffic days series", todayRow?.signups === 2);
  check("every other day in the window has signups: 0, not missing", stats.traffic.days.every((d) => typeof d.signups === "number"));
}

// --- with no account database (a server that never set one up) -------------
{
  const stats = runStats({ withDb: false });
  check("accounts is null, not an error, when the database doesn't exist", stats.accounts === null);
  check("still writes a valid stats.json", typeof stats.generated === "string");
  check("days still carry signups: 0 rather than being dropped", stats.traffic.days.every((d) => d.signups === 0));
}

console.log(failed ? `\n${failed} test(s) FAILED` : "\nAll tests passed.");
process.exit(failed ? 1 : 0);
