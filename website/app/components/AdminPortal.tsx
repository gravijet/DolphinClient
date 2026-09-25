"use client";

// The operations portal. Everything on this page is MEASURED: the numbers come
// from `/admin-data/stats.json`, which a systemd timer regenerates every ten
// minutes from this server's own nginx logs, the published manifest and the
// changelog (deploy/admin-stats.mjs). Nothing is estimated or invented.
//
// The page itself is a plain static export — it holds no secrets. What protects
// it is the gate in front: Cloudflare Access at the edge and, at the origin,
// nginx checking the signed Access token on every request for /admin and
// /admin-data (deploy/nginx/dolphinclient-admin.conf).

import { useCallback, useEffect, useState } from "react";
import ChangelogAdmin from "./ChangelogAdmin";

interface Day {
  day: string;
  views: number;
  visitors: number;
  downloads: number;
  updateChecks: number;
  signups: number;
}
interface Counted {
  count: number;
  [k: string]: string | number;
}
interface Stats {
  generated: string;
  release: {
    version: string | null;
    minecraft: string | null;
    generatedAt: string | null;
    archived: number;
    platforms: { os: string; label: string; available: boolean }[];
    assets: { what: string; name: string; size: number; sha256: string; url: string | null }[];
  } | null;
  history: {
    v: string;
    headline: string;
    current: boolean;
    items: number;
    shots: number;
    published: string | null;
  }[];
  traffic: {
    windowDays: number;
    logFiles: number;
    logLines: number;
    requests: number;
    lastRequest: string | null;
    dedicatedLogFrom: string | null;
    botHits: number;
    days: Day[];
    downloads: { total: number; bytes: number; byAsset: Counted[]; byVersion: Counted[] };
    updateChecks: number;
    topPages: Counted[];
    topReferrers: Counted[];
    topAgents: Counted[];
    errors: Counted[];
  };
  accounts: {
    total: number;
    verified: number;
    withMinecraft: number;
    activeSessions: number;
  } | null;
  system: {
    nginx: string;
    accessGate: string;
    disk: { free: number; total: number } | null;
    webrootBytes: number | null;
    downloadsBytes: number | null;
    load: number[] | null;
    uptimeSeconds: number | null;
    cert: { name: string; subject: string; validTo: string } | null;
    hostname: string | null;
    git: { head: string | null; subject: string | null; when: string | null; dirty: boolean };
    deployedAt: string | null;
  };
}

const bytes = (n?: number | null) => {
  if (n === null || n === undefined) return "—";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 && i > 0 ? 1 : 0)} ${u[i]}`;
};

const when = (iso?: string | null) => {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toLocaleString("en-GB", {
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
};

const ago = (iso?: string | null) => {
  if (!iso) return "—";
  const secs = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000);
  if (secs < 90) return `${Math.round(secs)} s ago`;
  if (secs < 5400) return `${Math.round(secs / 60)} min ago`;
  if (secs < 172800) return `${Math.round(secs / 3600)} h ago`;
  return `${Math.round(secs / 86400)} days ago`;
};

const duration = (secs?: number | null) => {
  if (!secs) return "—";
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  return d ? `${d} d ${h} h` : `${h} h`;
};

/** Days until a date, negative once it has passed. */
const daysUntil = (iso?: string | null) =>
  iso ? Math.round((new Date(iso).getTime() - Date.now()) / 86400000) : null;

/** A small bar chart. No chart library — an SVG with one rect per day. */
function Bars({
  days,
  pick,
  label,
  accent,
}: {
  days: Day[];
  pick: (d: Day) => number;
  label: string;
  accent?: boolean;
}) {
  const values = days.map(pick);
  const max = Math.max(1, ...values);
  const total = values.reduce((a, b) => a + b, 0);
  const w = 4;
  const gap = 2;
  const h = 64;
  return (
    <div className="adm-chart">
      <div className="adm-chart__head">
        <span>{label}</span>
        <b>{total.toLocaleString("en-GB")}</b>
      </div>
      <svg
        viewBox={`0 0 ${days.length * (w + gap)} ${h}`}
        preserveAspectRatio="none"
        role="img"
        aria-label={`${label}: ${total} over ${days.length} days`}
      >
        {days.map((d, i) => {
          const v = pick(d);
          const bh = Math.max(v > 0 ? 2 : 0.6, (v / max) * h);
          return (
            <rect
              key={d.day}
              x={i * (w + gap)}
              y={h - bh}
              width={w}
              height={bh}
              rx={1.2}
              className={accent ? "is-accent" : undefined}
            >
              <title>{`${d.day}: ${v}`}</title>
            </rect>
          );
        })}
      </svg>
      <div className="adm-chart__foot">
        <span>{days[0]?.day}</span>
        <span>{days[days.length - 1]?.day}</span>
      </div>
    </div>
  );
}

function Table({
  head,
  rows,
  empty,
}: {
  head: [string, string];
  rows: { key: string; count: number }[];
  empty: string;
}) {
  return (
    <table className="adm-table">
      <thead>
        <tr>
          <th>{head[0]}</th>
          <th className="num">{head[1]}</th>
        </tr>
      </thead>
      <tbody>
        {rows.length ? (
          rows.map((r) => (
            <tr key={r.key}>
              <td title={r.key}>{r.key}</td>
              <td className="num">{r.count.toLocaleString("en-GB")}</td>
            </tr>
          ))
        ) : (
          <tr>
            <td colSpan={2} className="adm-empty">
              {empty}
            </td>
          </tr>
        )}
      </tbody>
    </table>
  );
}

export default function AdminPortal() {
  const [stats, setStats] = useState<Stats | null>(null);
  const [email, setEmail] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  // The tab lives in the URL hash, so /admin#changelog opens the editor
  // directly and a reload keeps you where you were.
  const [view, setView] = useState<"overview" | "changelog">("overview");
  useEffect(() => {
    const fromHash = () =>
      setView(window.location.hash === "#changelog" ? "changelog" : "overview");
    fromHash();
    window.addEventListener("hashchange", fromHash);
    return () => window.removeEventListener("hashchange", fromHash);
  }, []);
  const show = (v: "overview" | "changelog") => {
    setView(v);
    window.location.hash = v === "changelog" ? "#changelog" : "";
  };

  const load = useCallback(() => {
    setLoading(true);
    fetch("/admin-data/stats.json", { cache: "no-store" })
      .then((r) => (r.ok ? r.json() : Promise.reject(new Error(`stats.json → HTTP ${r.status}`))))
      .then((d: Stats) => {
        setStats(d);
        setError(null);
      })
      .catch((e: Error) => setError(e.message))
      .finally(() => setLoading(false));
    fetch("/admin-data/whoami.json", { cache: "no-store" })
      .then((r) => (r.ok ? r.json() : null))
      .then((d) => setEmail(d?.email || null))
      .catch(() => setEmail(null));
  }, []);

  useEffect(load, [load]);

  const t = stats?.traffic;
  const sys = stats?.system;
  const certDays = daysUntil(sys?.cert?.validTo);

  return (
    <main className="adm">
      <header className="adm-head">
        <div>
          <span className="kicker">Admin · operations</span>
          <h1>
            Control <span className="accent">room</span>
          </h1>
          <p className="adm-lede">
            Everything here is measured on this server — nginx logs, the published
            manifest, the changelog. Refreshed every ten minutes.
          </p>
        </div>
        <div className="adm-id">
          {email && <span className="adm-pill">{email}</span>}
          <span className="adm-when">
            {stats ? `Data ${ago(stats.generated)}` : loading ? "Loading…" : "No data"}
          </span>
          <button type="button" className="btn ghost sm" onClick={load} disabled={loading}>
            {loading ? "Refreshing…" : "Refresh"}
          </button>
        </div>
      </header>

      <nav className="adm-tabs" aria-label="Sections">
        <button
          type="button"
          className={view === "overview" ? "is-on" : undefined}
          onClick={() => show("overview")}
        >
          Overview
        </button>
        <button
          type="button"
          className={view === "changelog" ? "is-on" : undefined}
          onClick={() => show("changelog")}
        >
          Changelog
        </button>
      </nav>

      {view === "changelog" && <ChangelogAdmin />}

      {view === "overview" && error && (
        <div className="adm-error">
          <b>Could not read the statistics.</b> {error}. The file is written by{" "}
          <code>dolphinclient-admin-stats.timer</code> — check{" "}
          <code>systemctl status dolphinclient-admin-stats</code>.
        </div>
      )}

      {view === "overview" && stats && t && sys && (
        <>
          <section className="adm-cards">
            <div className="adm-card">
              <span className="adm-card__k">Live version</span>
              <b className="adm-card__v accent">{stats.release?.version ?? "—"}</b>
              <span className="adm-card__s">
                for Minecraft {stats.release?.minecraft ?? "—"} · {stats.release?.archived ?? 0}{" "}
                builds archived
              </span>
            </div>
            <div className="adm-card">
              <span className="adm-card__k">Downloads · {t.windowDays} d</span>
              <b className="adm-card__v">{t.downloads.total.toLocaleString("en-GB")}</b>
              <span className="adm-card__s">{bytes(t.downloads.bytes)} sent</span>
            </div>
            <div className="adm-card">
              <span className="adm-card__k">Update checks · {t.windowDays} d</span>
              <b className="adm-card__v">{t.updateChecks.toLocaleString("en-GB")}</b>
              <span className="adm-card__s">launchers asking for the manifest</span>
            </div>
            <div className="adm-card">
              <span className="adm-card__k">Visitors · {t.windowDays} d</span>
              <b className="adm-card__v">
                {t.days.reduce((a, d) => a + d.visitors, 0).toLocaleString("en-GB")}
              </b>
              <span className="adm-card__s">
                {t.days.reduce((a, d) => a + d.views, 0).toLocaleString("en-GB")} page views ·{" "}
                {t.botHits.toLocaleString("en-GB")} bot hits ignored
              </span>
            </div>
            <div className="adm-card">
              <span className="adm-card__k">Accounts</span>
              <b className="adm-card__v">
                {stats.accounts ? stats.accounts.total.toLocaleString("en-GB") : "—"}
              </b>
              <span className="adm-card__s">
                {stats.accounts
                  ? `${stats.accounts.verified.toLocaleString("en-GB")} verified · ${stats.accounts.withMinecraft.toLocaleString("en-GB")} linked · ${stats.accounts.activeSessions.toLocaleString("en-GB")} active sessions`
                  : "no account database found"}
              </span>
            </div>
          </section>

          <section className="adm-panel">
            <h2>Last {t.windowDays} days</h2>
            <div className="adm-charts">
              <Bars days={t.days} pick={(d) => d.views} label="Page views" />
              <Bars days={t.days} pick={(d) => d.visitors} label="Unique visitors" />
              <Bars days={t.days} pick={(d) => d.downloads} label="Downloads" accent />
              <Bars days={t.days} pick={(d) => d.updateChecks} label="Update checks" />
              <Bars days={t.days} pick={(d) => d.signups} label="New accounts" />
            </div>
            <p className="adm-note">
              Read from {t.logFiles} log files ({t.logLines.toLocaleString("en-GB")} lines,{" "}
              {t.requests.toLocaleString("en-GB")} of them this site). Last request{" "}
              {ago(t.lastRequest)}.
              {t.dedicatedLogFrom
                ? ` This site has had its own log since ${when(t.dedicatedLogFrom)}.`
                : " Until this site's own log fills up, days are taken from the host's shared log and matched by path, so the home page count may include another site on this host."}
            </p>
          </section>

          <section className="adm-grid">
            <div className="adm-panel">
              <h2>Current release</h2>
              <table className="adm-table">
                <thead>
                  <tr>
                    <th>Asset</th>
                    <th className="num">Size</th>
                    <th>SHA-256</th>
                  </tr>
                </thead>
                <tbody>
                  {stats.release?.assets.map((a) => (
                    <tr key={a.what}>
                      <td>
                        <b>{a.what}</b>
                        <span className="adm-sub">{a.name}</span>
                      </td>
                      <td className="num">{bytes(a.size)}</td>
                      <td className="mono">{a.sha256}…</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div className="adm-chips">
                {stats.release?.platforms.map((p) => (
                  <span key={p.os} className={`adm-chip${p.available ? " is-on" : ""}`}>
                    {p.label} {p.available ? "live" : "not built"}
                  </span>
                ))}
              </div>
              <p className="adm-note">Manifest written {when(stats.release?.generatedAt)}.</p>
            </div>

            <div className="adm-panel">
              <h2>Server</h2>
              <dl className="adm-dl">
                <dt>nginx</dt>
                <dd className={sys.nginx === "active" ? "good" : "warn"}>{sys.nginx}</dd>
                <dt>Access gate</dt>
                <dd className={sys.accessGate === "active" ? "good" : "warn"}>{sys.accessGate}</dd>
                <dt>Disk free</dt>
                <dd className={sys.disk && sys.disk.free < 5e9 ? "warn" : ""}>
                  {bytes(sys.disk?.free)} of {bytes(sys.disk?.total)}
                </dd>
                <dt>Downloads folder</dt>
                <dd>{bytes(sys.downloadsBytes)}</dd>
                <dt>TLS certificate</dt>
                <dd className={certDays !== null && certDays < 14 ? "warn" : ""}>
                  {sys.cert ? `${sys.cert.subject} · ${certDays} days left` : "—"}
                </dd>
                <dt>Uptime</dt>
                <dd>
                  {duration(sys.uptimeSeconds)}
                  {sys.load ? ` · load ${sys.load.join(" / ")}` : ""}
                </dd>
                <dt>Website deployed</dt>
                <dd>{when(sys.deployedAt)}</dd>
                <dt>Repo HEAD</dt>
                <dd className="mono">
                  {sys.git.head ?? "—"} {sys.git.dirty ? "· uncommitted changes" : ""}
                  <span className="adm-sub">{sys.git.subject}</span>
                </dd>
              </dl>
            </div>
          </section>

          <section className="adm-grid">
            <div className="adm-panel">
              <h2>Downloads by asset</h2>
              <Table
                head={["Asset", "Count"]}
                rows={t.downloads.byAsset.map((r) => ({ key: String(r.asset), count: r.count }))}
                empty="No download has been logged yet in this window."
              />
            </div>
            <div className="adm-panel">
              <h2>Downloads by client version</h2>
              <Table
                head={["Version", "Count"]}
                rows={t.downloads.byVersion.map((r) => ({
                  key: String(r.version),
                  count: r.count,
                }))}
                empty="Nothing from the version archive yet."
              />
            </div>
          </section>

          <section className="adm-grid">
            <div className="adm-panel">
              <h2>Top pages</h2>
              <Table
                head={["Path", "Views"]}
                rows={t.topPages.map((r) => ({ key: String(r.path), count: r.count }))}
                empty="No page views recorded."
              />
            </div>
            <div className="adm-panel">
              <h2>Where they came from</h2>
              <Table
                head={["Referrer", "Views"]}
                rows={t.topReferrers.map((r) => ({ key: String(r.referrer), count: r.count }))}
                empty="Everyone arrived without a referrer."
              />
            </div>
            <div className="adm-panel">
              <h2>Browsers</h2>
              <Table
                head={["Agent", "Views"]}
                rows={t.topAgents.map((r) => ({ key: String(r.agent), count: r.count }))}
                empty="No agents recorded."
              />
            </div>
            <div className="adm-panel">
              <h2>Errors</h2>
              <Table
                head={["Status and path", "Hits"]}
                rows={t.errors.map((r) => ({ key: String(r.what), count: r.count }))}
                empty="No 4xx or 5xx in this window."
              />
            </div>
          </section>

          <section className="adm-panel">
            <h2>Releases</h2>
            <table className="adm-table">
              <thead>
                <tr>
                  <th>Version</th>
                  <th>Headline</th>
                  <th className="num">Items</th>
                  <th className="num">Shots</th>
                  <th>Published</th>
                </tr>
              </thead>
              <tbody>
                {stats.history.map((h) => (
                  <tr key={h.v} className={h.current ? "is-current" : undefined}>
                    <td className="mono">{h.v}</td>
                    <td>{h.headline}</td>
                    <td className="num">{h.items}</td>
                    <td className="num">{h.shots || "—"}</td>
                    <td>{when(h.published)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>

          <p className="adm-foot">
            Generated {when(stats.generated)} by <code>deploy/admin-stats.mjs</code> · protected by
            Cloudflare Access and the origin token gate.
          </p>
        </>
      )}
    </main>
  );
}
