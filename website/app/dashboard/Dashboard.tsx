"use client";

import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import Counter from "../components/Counter";

/* ------------------------------------------------------------------ */
/*  The dashboard reads live state directly from the running launcher. */
/*  The launcher exposes a tiny loopback HTTP endpoint; loopback       */
/*  origins are "potentially trustworthy", so an HTTPS page may fetch  */
/*  them, and the launcher answers the CORS + Private-Network preflight.*/
/* ------------------------------------------------------------------ */

const BRIDGE = "http://127.0.0.1:47654/status";
const MANIFEST = "/downloads/manifest.json";

interface Account {
  name: string;
  uuid: string;
  source: string;
}
interface LauncherSettings {
  server: string;
  ramGb: number;
  fullscreen: boolean;
  autoUpdate: boolean;
  closeOnLaunch: boolean;
  cape: string;
}
interface ServerInfo {
  name: string;
  address: string;
  default: boolean;
}
interface GameOpts {
  renderDistance: number;
  maxFps: number;
  vsync: boolean;
  fov: number;
  guiScale: number;
  graphics: string;
  discordRpc: boolean;
}
interface SessionInfo {
  at: number;
  secs: number;
}
interface LauncherStatus {
  connected: boolean;
  launcherVersion: string;
  minecraft: string;
  clientVersion: string;
  running: boolean;
  status: string;
  account: Account | null;
  accounts: number;
  settings: LauncherSettings;
  servers?: ServerInfo[];
  game?: GameOpts;
  stats: {
    playtimeSecs: number;
    launches: number;
    lastPlayed: number | null;
    avgSessionSecs?: number;
    sessions?: SessionInfo[];
  };
}
interface Platform {
  available: boolean;
  label: string;
  url?: string;
  size?: number;
  sha256?: string;
}
interface Manifest {
  version: string;
  minecraft?: string;
  platforms: Record<string, Platform>;
  client?: Record<string, { version?: string; sha256?: string; size?: number }>;
}

const CAPES: { id: string; name: string; from: string; to: string }[] = [
  { id: "", name: "Keine Cape", from: "#2a3348", to: "#1a2233" },
  { id: "dolphin", name: "Dolphin", from: "#35e0c8", to: "#1e8ca8" },
  { id: "ocean", name: "Ozean", from: "#4f8cff", to: "#243a8c" },
  { id: "aurora", name: "Aurora", from: "#9b7bff", to: "#538be0" },
  { id: "magma", name: "Magma", from: "#ff8a4f", to: "#c02e3a" },
  { id: "founder", name: "Founder", from: "#ffc45a", to: "#c98622" },
];

function fmtPlaytime(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}
function fmtRelative(unix: number | null): string {
  if (!unix) return "—";
  const diff = Date.now() / 1000 - unix;
  if (diff < 90) return "gerade eben";
  if (diff < 3600) return `vor ${Math.round(diff / 60)} Min`;
  if (diff < 86400) return `vor ${Math.round(diff / 3600)} Std`;
  return `vor ${Math.round(diff / 86400)} Tagen`;
}
function fmtSize(bytes?: number): string {
  return bytes ? (bytes / 1024 / 1024).toFixed(1) + " MB" : "";
}

/* ------------------------------------------------------------------ */
/*  Icons                                                              */
/* ------------------------------------------------------------------ */

const I = {
  grid: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/></svg>,
  cape: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M6 3v10c0 3 2.7 5 6 5s6-2 6-5V3"/><path d="M6 3c0 2 2.7 3 6 3s6-1 6-3"/></svg>,
  download: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/></svg>,
  cog: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-2.9 1.2V21a2 2 0 1 1-4 0v-.1A1.7 1.7 0 0 0 7 19.4a1.7 1.7 0 0 0-1.9.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0-1.2-2.9H1a2 2 0 1 1 0-4h.1A1.7 1.7 0 0 0 2.6 7a1.7 1.7 0 0 0-.3-1.9l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1A1.7 1.7 0 0 0 9 2.6a1.7 1.7 0 0 0 1-.5"/></svg>,
  clock: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>,
  play: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M6 4l14 8-14 8V4Z"/></svg>,
  user: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="8" r="4"/><path d="M4 21c0-4 4-6 8-6s8 2 8 6"/></svg>,
  check: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"><path d="m20 6-11 11-5-5"/></svg>,
  server: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><rect x="3" y="4" width="18" height="7" rx="2"/><rect x="3" y="13" width="18" height="7" rx="2"/><path d="M7 7.5h.01M7 16.5h.01"/></svg>,
  sliders: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0"/><circle cx="16" cy="6" r="2"/><circle cx="10" cy="12" r="2"/><circle cx="18" cy="18" r="2"/></svg>,
};

type Tab = "overview" | "servers" | "cosmetics" | "downloads" | "settings";
const TABS: { id: Tab; label: string; icon: JSX.Element }[] = [
  { id: "overview", label: "Übersicht", icon: I.grid },
  { id: "servers", label: "Server", icon: I.server },
  { id: "cosmetics", label: "Cosmetics", icon: I.cape },
  { id: "downloads", label: "Downloads", icon: I.download },
  { id: "settings", label: "Einstellungen", icon: I.cog },
];

/* ------------------------------------------------------------------ */
/*  Component                                                          */
/* ------------------------------------------------------------------ */

export default function Dashboard() {
  const [tab, setTab] = useState<Tab>("overview");
  const [status, setStatus] = useState<LauncherStatus | null>(null);
  const [connected, setConnected] = useState<boolean | null>(null);
  const [manifest, setManifest] = useState<Manifest | null>(null);

  /* poll the running launcher */
  useEffect(() => {
    let alive = true;
    async function poll() {
      try {
        const r = await fetch(BRIDGE, { cache: "no-store" });
        const d = (await r.json()) as LauncherStatus;
        if (alive) {
          setStatus(d);
          setConnected(true);
        }
      } catch {
        if (alive) setConnected(false);
      }
    }
    poll();
    const iv = setInterval(poll, 3000);
    return () => {
      alive = false;
      clearInterval(iv);
    };
  }, []);

  /* load the download manifest once */
  useEffect(() => {
    fetch(MANIFEST, { cache: "no-store" })
      .then((r) => (r.ok ? r.json() : Promise.reject()))
      .then(setManifest)
      .catch(() => void 0);
  }, []);

  const account = status?.account ?? null;
  const initials = (account?.name ?? "??").slice(0, 2).toUpperCase();
  const capeName =
    CAPES.find((c) => c.id === (status?.settings.cape ?? ""))?.name ?? "Keine Cape";

  const conn = useMemo(() => {
    if (connected === null) return { cls: "wait", text: "Suche Launcher …" };
    if (connected && status?.running)
      return { cls: "live", text: "Verbunden · Spiel läuft" };
    if (connected) return { cls: "ok", text: "Launcher verbunden" };
    return { cls: "off", text: "Launcher offline" };
  }, [connected, status]);

  /* ---------------------------------------------------------------- */
  /*  Not connected → gate (still shows downloads)                     */
  /* ---------------------------------------------------------------- */
  if (connected === false) {
    return (
      <div className="panel dash-gate">
        <span className={`conn-badge off`}>
          <span className="conn-dot" /> {conn.text}
        </span>
        <h3 className="panel-title">Starte den DolphinClient-Launcher</h3>
        <p className="sub">
          Dieses Dashboard hängt lokal am laufenden Launcher (127.0.0.1) — ohne
          Konto-Anmeldung, ohne Umweg über einen Server. Es liest live dein
          aktives Konto, die Version, deine Spielzeit und Einstellungen.
        </p>
        <ol className="dash-steps">
          <li>Launcher beziehen und installieren.</li>
          <li>Launcher öffnen und mit Microsoft anmelden.</li>
          <li>Diese Seite verbindet sich von selbst — kein Neuladen nötig.</li>
        </ol>
        <div className="cta">
          <Link className="btn" href="/download">
            Launcher beziehen
          </Link>
        </div>
        {manifest && (
          <p className="sub" style={{ marginTop: "1rem" }}>
            Neueste Build:&nbsp;
            <span className="tag">v{manifest.version}</span> · Minecraft{" "}
            {manifest.minecraft ?? "26.1"}
          </p>
        )}
      </div>
    );
  }

  /* ---------------------------------------------------------------- */
  /*  Connected (or still probing) dashboard                          */
  /* ---------------------------------------------------------------- */
  const st = status;
  return (
    <div className="dash">
      <aside className="dash__side">
        <div className="dash__profile">
          <div className="dash__avatar">
            {account ? (
              // eslint-disable-next-line @next/next/no-img-element
              <img
                src={`https://minotar.net/helm/${account.name}/64.png`}
                alt=""
                width={44}
                height={44}
                style={{ borderRadius: 10 }}
              />
            ) : (
              initials
            )}
          </div>
          <div>
            <b>{account?.name ?? "Kein Konto"}</b>
            <small className={conn.cls === "off" ? "" : "online"}>
              ● {account ? "Aktiv" : "Nicht angemeldet"}
            </small>
          </div>
        </div>
        <nav className="dash__nav">
          {TABS.map((t) => (
            <button
              key={t.id}
              className={tab === t.id ? "active" : ""}
              onClick={() => setTab(t.id)}
            >
              {t.icon}
              {t.label}
            </button>
          ))}
        </nav>
        <div className={`conn-badge ${conn.cls}`} style={{ marginTop: "0.8rem" }}>
          <span className="conn-dot" /> {conn.text}
        </div>
      </aside>

      <div className="dash__main">
        {tab === "overview" && (
          <div className="dash__section">
            <div className="stat-grid">
              <StatCard icon={I.clock} num={<Counter to={Math.round((st?.stats.playtimeSecs ?? 0) / 60)} suffix=" min" />} lbl="Spielzeit" />
              <StatCard icon={I.play} num={<Counter to={st?.stats.launches ?? 0} />} lbl="Starts" />
              <StatCard icon={I.download} num={st?.clientVersion ?? "—"} lbl="Client-Version" />
              <StatCard icon={I.user} num={<Counter to={st?.accounts ?? 0} />} lbl="Konten" />
            </div>

            <div className="grid-2" style={{ marginTop: "1.1rem" }}>
              <div className="panel" style={{ margin: 0 }}>
                <h3 className="panel-title">Aktives Konto</h3>
                {account ? (
                  <ul className="feed">
                    <FeedItem icon={I.user} title={account.name} sub={account.source} time="aktiv" />
                    <FeedItem icon={I.play} title={st?.running ? "Spiel läuft" : "Bereit"} sub={`Minecraft ${st?.minecraft ?? "26.1"}`} time={st?.running ? "live" : ""} />
                    <FeedItem icon={I.cape} title={capeName} sub="Ausgewählte Cape" time="" />
                  </ul>
                ) : (
                  <p className="sub">Im Launcher noch kein Konto ausgewählt.</p>
                )}
              </div>

              <div className="panel" style={{ margin: 0 }}>
                <h3 className="panel-title">Status</h3>
                <ul className="feed">
                  <FeedItem icon={I.clock} title="Zuletzt gespielt" sub="" time={fmtRelative(st?.stats.lastPlayed ?? null)} />
                  <FeedItem icon={I.download} title="Launcher" sub="Nativ · Rust" time={`v${st?.launcherVersion ?? "—"}`} />
                  <FeedItem icon={I.check} title={st?.status ?? "—"} sub="Launcher meldet" time="" />
                </ul>
              </div>
            </div>

            <div className="panel" style={{ margin: "1.1rem 0 0" }}>
              <div style={{ display: "flex", alignItems: "baseline", gap: "0.6rem" }}>
                <h3 className="panel-title">Aktivität</h3>
                <span className="sub" style={{ margin: 0, marginLeft: "auto" }}>
                  Ø {fmtPlaytime(st?.stats.avgSessionSecs ?? 0)} · Gesamt{" "}
                  {fmtPlaytime(st?.stats.playtimeSecs ?? 0)}
                </span>
              </div>
              <Sparkline sessions={st?.stats.sessions ?? []} />
            </div>
          </div>
        )}

        {tab === "servers" && (
          <div className="dash__section panel" style={{ margin: 0 }}>
            <h3 className="panel-title">Server</h3>
            <p className="sub">
              Deine im Launcher gespeicherten Server. Hinzufügen und Beitreten
              machst du im Launcher (Tab „Server“).
            </p>
            {(st?.servers?.length ?? 0) === 0 ? (
              <p className="sub">Noch keine Server gespeichert.</p>
            ) : (
              <ul className="feed">
                {st!.servers!.map((sv) => (
                  <FeedItem
                    key={sv.address}
                    icon={I.server}
                    title={sv.name}
                    sub={sv.address}
                    time={sv.default ? "★ Standard" : ""}
                  />
                ))}
              </ul>
            )}
          </div>
        )}

        {tab === "cosmetics" && (
          <div className="dash__section panel" style={{ margin: 0 }}>
            <h3 className="panel-title">Cosmetics</h3>
            <p className="sub">
              Deine Cape wählst du direkt im Launcher (Tab „Cosmetics“). Hier siehst
              du deine aktuelle Auswahl — die In-Game-Darstellung folgt in einem Update.
            </p>
            <div className="cape-grid">
              {CAPES.map((cape) => {
                const active = (st?.settings.cape ?? "") === cape.id;
                return (
                  <div key={cape.id} className={`cape${active ? " active" : ""}`}>
                    <div
                      className="cape__swatch"
                      style={{ background: `linear-gradient(160deg, ${cape.from}, ${cape.to})` }}
                    />
                    <h3>{cape.name}</h3>
                    <p>{active ? "Aktiv" : "Im Launcher wählbar"}</p>
                  </div>
                );
              })}
            </div>
          </div>
        )}

        {tab === "downloads" && (
          <div className="dash__section panel" style={{ margin: 0 }}>
            <h3 className="panel-title">Downloads &amp; Version</h3>
            <p className="sub">
              Live aus dem Veröffentlichungs-Manifest. Der Launcher hält Client und
              sich selbst automatisch aktuell.
            </p>
            <ul className="feed" style={{ marginBottom: "1.2rem" }}>
              <FeedItem
                icon={I.download}
                title="DolphinClient-Launcher"
                sub={st ? (st.launcherVersion === manifest?.version ? "Aktuell" : "Update verfügbar") : "Nativ · Rust"}
                time={manifest ? `v${manifest.version}` : "…"}
              />
              <FeedItem icon={I.play} title="Native Engine" sub="Rust · wgpu — kein Java/Fabric" time={`MC ${manifest?.minecraft ?? "26.1"}`} />
              {manifest?.platforms &&
                Object.entries(manifest.platforms)
                  .filter(([, p]) => p.available)
                  .map(([os, p]) => (
                    <FeedItem key={os} icon={I.download} title={p.label} sub={p.sha256 ? `SHA-256 ${p.sha256.slice(0, 12)}…` : ""} time={fmtSize(p.size)} />
                  ))}
            </ul>
            <div className="cta">
              <Link className="btn" href="/download">Zur Download-Seite</Link>
              <Link className="btn ghost" href="/download#changelog">Changelog</Link>
            </div>
          </div>
        )}

        {tab === "settings" && (
          <div className="dash__section panel" style={{ margin: 0 }}>
            <h3 className="panel-title">Launcher-Einstellungen</h3>
            <p className="sub">
              Live vom Launcher. Änderungen machst du in der Launcher-App — dort
              werden sie automatisch gespeichert.
            </p>
            <ReadRow label="Standard-Server" value={st?.settings.server || "Serverauswahl im Spiel"} />
            <ReadRow label="Zugewiesener RAM" value={`${st?.settings.ramGb ?? "—"} GB`} />
            <ReadRow label="Auto-Update" value={st?.settings.autoUpdate ? "An" : "Aus"} on={st?.settings.autoUpdate} />
            <ReadRow label="Vollbild starten" value={st?.settings.fullscreen ? "An" : "Aus"} on={st?.settings.fullscreen} />
            <ReadRow label="Launcher nach Start schließen" value={st?.settings.closeOnLaunch ? "An" : "Aus"} on={st?.settings.closeOnLaunch} />

            {st?.game && (
              <>
                <h3 className="panel-title" style={{ marginTop: "1.6rem" }}>Spiel-Einstellungen</h3>
                <p className="sub">
                  Live aus der options.json des Clients — im Launcher unter
                  „Einstellungen“ änderbar.
                </p>
                <ReadRow label="Render-Distanz" value={`${st.game.renderDistance} Chunks`} />
                <ReadRow label="Max. FPS" value={st.game.maxFps > 0 ? `${st.game.maxFps} FPS` : "Unbegrenzt"} />
                <ReadRow label="VSync" value={st.game.vsync ? "An" : "Aus"} on={st.game.vsync} />
                <ReadRow label="Sichtfeld (FoV)" value={`${Math.round(st.game.fov)}°`} />
                <ReadRow label="GUI-Skalierung" value={st.game.guiScale === 0 ? "Auto" : `${st.game.guiScale}×`} />
                <ReadRow label="Grafik" value={st.game.graphics} />
                <ReadRow label="Discord Rich Presence" value={st.game.discordRpc ? "An" : "Aus"} on={st.game.discordRpc} />
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/*  Presentational helpers                                             */
/* ------------------------------------------------------------------ */

function StatCard({ icon, num, lbl }: { icon: JSX.Element; num: React.ReactNode; lbl: string }) {
  return (
    <div className="stat-card">
      <span className="ico">{icon}</span>
      <div className="num">{num}</div>
      <div className="lbl">{lbl}</div>
    </div>
  );
}

function FeedItem({ icon, title, sub, time }: { icon: JSX.Element; title: string; sub: string; time: string }) {
  return (
    <li>
      <span className="fico">{icon}</span>
      <div>
        <b>{title}</b>
        {sub && <small>{sub}</small>}
      </div>
      {time && <time>{time}</time>}
    </li>
  );
}

function Sparkline({ sessions }: { sessions: SessionInfo[] }) {
  if (!sessions.length) {
    return (
      <p className="sub" style={{ margin: "0.8rem 0 0" }}>
        Noch keine Sitzungen — starte das Spiel im Launcher, um deine Historie zu
        sehen.
      </p>
    );
  }
  const max = Math.max(...sessions.map((s) => s.secs), 1);
  return (
    <div className="spark">
      {sessions.map((s, i) => {
        const h = Math.max(6, Math.round((s.secs / max) * 100));
        return (
          <span
            key={i}
            className={`spark__bar${i === sessions.length - 1 ? " is-last" : ""}`}
            style={{ height: `${h}%` }}
            title={fmtPlaytime(s.secs)}
          />
        );
      })}
    </div>
  );
}

function ReadRow({ label, value, on }: { label: string; value: string; on?: boolean }) {
  return (
    <div className="toggle-row">
      <div className="meta">
        <b>{label}</b>
      </div>
      <span className={`tag${on ? " on" : ""}`}>{value}</span>
    </div>
  );
}
