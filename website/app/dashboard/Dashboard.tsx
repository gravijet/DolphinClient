"use client";

import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import Counter from "../components/Counter";

/* ------------------------------------------------------------------ */
/*  Types + config                                                     */
/* ------------------------------------------------------------------ */

interface Cape {
  id: string;
  name: string;
  textureUrl: string;
}

interface Identity {
  username: string;
  uuid: string;
}

interface Settings {
  ram: number;
  autoUpdate: boolean;
  showFps: boolean;
  showCoords: boolean;
  keystrokes: boolean;
  fullscreen: boolean;
}

const API_BASE = process.env.NEXT_PUBLIC_API_BASE ?? "http://localhost:3001/v1";
const LS_ID = "dolphin.identity";
const LS_SET = "dolphin.settings";

const DEFAULT_SETTINGS: Settings = {
  ram: 4,
  autoUpdate: true,
  showFps: true,
  showCoords: true,
  keystrokes: false,
  fullscreen: false,
};

/* deterministic hash → stable pseudo-stats per player (no random) */
function hash(str: string): number {
  let h = 2166136261;
  for (let i = 0; i < str.length; i++) {
    h ^= str.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

/* ------------------------------------------------------------------ */
/*  Icons                                                              */
/* ------------------------------------------------------------------ */

const I = {
  grid: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/></svg>,
  cape: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M6 3v10c0 3 2.7 5 6 5s6-2 6-5V3"/><path d="M6 3c0 2 2.7 3 6 3s6-1 6-3"/></svg>,
  chart: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M3 3v18h18"/><path d="M7 15l3-4 3 3 4-6"/></svg>,
  download: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/></svg>,
  cog: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-2.9 1.2V21a2 2 0 1 1-4 0v-.1A1.7 1.7 0 0 0 7 19.4a1.7 1.7 0 0 0-1.9.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0-1.2-2.9H1a2 2 0 1 1 0-4h.1A1.7 1.7 0 0 0 2.6 7a1.7 1.7 0 0 0-.3-1.9l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1A1.7 1.7 0 0 0 9 2.6a1.7 1.7 0 0 0 1-.5"/></svg>,
  clock: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>,
  bolt: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M13 2 3 14h9l-1 8 10-12h-9l1-8Z"/></svg>,
  play: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M6 4l14 8-14 8V4Z"/></svg>,
  check: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"><path d="m20 6-11 11-5-5"/></svg>,
};

type Tab = "overview" | "cosmetics" | "stats" | "downloads" | "settings";

const TABS: { id: Tab; label: string; icon: JSX.Element }[] = [
  { id: "overview", label: "Übersicht", icon: I.grid },
  { id: "cosmetics", label: "Cosmetics", icon: I.cape },
  { id: "stats", label: "Statistiken", icon: I.chart },
  { id: "downloads", label: "Downloads", icon: I.download },
  { id: "settings", label: "Einstellungen", icon: I.cog },
];

/* ------------------------------------------------------------------ */
/*  Component                                                          */
/* ------------------------------------------------------------------ */

export default function Dashboard() {
  const [identity, setIdentity] = useState<Identity | null>(null);
  const [formName, setFormName] = useState("");
  const [formUuid, setFormUuid] = useState("");
  const [tab, setTab] = useState<Tab>("overview");

  const [capes, setCapes] = useState<Cape[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);

  const [toast, setToast] = useState<{ msg: string; kind: "ok" | "err" } | null>(null);

  function notify(msg: string, kind: "ok" | "err" = "ok") {
    setToast({ msg, kind });
    window.setTimeout(() => setToast(null), 2600);
  }

  /* restore identity + settings */
  useEffect(() => {
    try {
      const id = localStorage.getItem(LS_ID);
      if (id) setIdentity(JSON.parse(id));
      const st = localStorage.getItem(LS_SET);
      if (st) setSettings({ ...DEFAULT_SETTINGS, ...JSON.parse(st) });
    } catch {
      /* ignore */
    }
  }, []);

  /* load available capes once */
  useEffect(() => {
    fetch(`${API_BASE}/cosmetics`)
      .then((r) => r.json())
      .then((d) => setCapes(d.capes ?? []))
      .catch(() => void 0);
  }, []);

  /* load active cape whenever identity changes */
  useEffect(() => {
    if (!identity) return;
    fetch(`${API_BASE}/cosmetics/${identity.uuid}`)
      .then((r) => r.json())
      .then((d) => setActiveId(d.cape?.id ?? null))
      .catch(() => void 0);
  }, [identity]);

  function connect(e: React.FormEvent) {
    e.preventDefault();
    const username = formName.trim() || "Spieler";
    const uuid = formUuid.trim() || `offline-${hash(username).toString(16)}`;
    const id = { username, uuid };
    setIdentity(id);
    localStorage.setItem(LS_ID, JSON.stringify(id));
    notify(`Angemeldet als ${username}`);
  }

  function disconnect() {
    setIdentity(null);
    setActiveId(null);
    localStorage.removeItem(LS_ID);
  }

  async function setActive(capeId: string | null) {
    if (!identity) return;
    try {
      const r = await fetch(`${API_BASE}/cosmetics/${identity.uuid}/active`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ capeId }),
      });
      const d = await r.json();
      if (d.error) return notify("Fehler: " + d.error, "err");
      setActiveId(d.activeCapeId ?? null);
      notify(capeId ? "Cape aktiviert." : "Cape entfernt.");
    } catch {
      notify("Backend nicht erreichbar.", "err");
    }
  }

  function saveSettings(next: Settings) {
    setSettings(next);
    localStorage.setItem(LS_SET, JSON.stringify(next));
  }

  /* derived pseudo-stats (stable per identity) */
  const stats = useMemo(() => {
    const seed = hash(identity?.uuid ?? "guest");
    return {
      hours: 40 + (seed % 260),
      sessions: 20 + (seed % 180),
      avgFps: 240 + (seed % 160),
      blocks: 50000 + (seed % 900000),
    };
  }, [identity]);

  const completeness = useMemo(() => {
    let n = 40;
    if (activeId) n += 25;
    if (settings.autoUpdate) n += 10;
    if (identity?.uuid && !identity.uuid.startsWith("offline")) n += 25;
    return Math.min(100, n);
  }, [activeId, settings, identity]);

  /* ---------------------------------------------------------------- */
  /*  Not connected → login gate                                      */
  /* ---------------------------------------------------------------- */
  if (!identity) {
    return (
      <div className="panel" style={{ maxWidth: 560 }}>
        <h3 className="panel-title">Beim Dashboard anmelden</h3>
        <p className="sub">
          Web-Login über Microsoft folgt. Bis dahin: Namen (und optional deine
          Spieler-UUID) eingeben — Cosmetics werden echt gegen die API gespeichert.
        </p>
        <form onSubmit={connect}>
          <div className="field">
            <input
              value={formName}
              onChange={(e) => setFormName(e.target.value)}
              placeholder="Spielername (z. B. Steve)"
              aria-label="Spielername"
            />
          </div>
          <div className="field">
            <input
              value={formUuid}
              onChange={(e) => setFormUuid(e.target.value)}
              placeholder="Spieler-UUID (optional)"
              aria-label="Spieler-UUID"
            />
          </div>
          <button className="btn" type="submit" style={{ marginTop: "0.6rem" }}>
            Verbinden
          </button>
        </form>
        {toast && (
          <div className={`toast ${toast.kind}`}>
            <span className="dot" />
            {toast.msg}
          </div>
        )}
      </div>
    );
  }

  const initials = identity.username.slice(0, 2).toUpperCase();

  /* ---------------------------------------------------------------- */
  /*  Connected dashboard                                             */
  /* ---------------------------------------------------------------- */
  return (
    <div className="dash">
      {/* hidden gradient def for progress rings */}
      <svg width="0" height="0" style={{ position: "absolute" }}>
        <defs>
          <linearGradient id="ringGrad" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor="#38e1c4" />
            <stop offset="1" stopColor="#7c8bff" />
          </linearGradient>
        </defs>
      </svg>

      {/* ---------- sidebar ---------- */}
      <aside className="dash__side">
        <div className="dash__profile">
          <div className="dash__avatar">{initials}</div>
          <div>
            <b>{identity.username}</b>
            <small>● Online</small>
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
        <button
          className="btn ghost sm"
          style={{ width: "100%", marginTop: "0.8rem" }}
          onClick={disconnect}
        >
          Abmelden
        </button>
      </aside>

      {/* ---------- main ---------- */}
      <div className="dash__main">
        {tab === "overview" && (
          <div className="dash__section">
            <div className="stat-grid">
              <StatCard icon={I.clock} num={<Counter to={stats.hours} suffix=" h" />} lbl="Spielzeit" trend="+12h" />
              <StatCard icon={I.play} num={<Counter to={stats.sessions} />} lbl="Sessions" trend="+3" />
              <StatCard icon={I.bolt} num={<Counter to={stats.avgFps} />} lbl="Ø FPS" trend="stabil" />
              <StatCard icon={I.cape} num={<Counter to={capes.length} />} lbl="Cosmetics" />
            </div>

            <div className="grid-2" style={{ marginTop: "1.1rem" }}>
              <div className="panel" style={{ margin: 0, display: "flex", gap: "1.2rem", alignItems: "center" }}>
                <Ring value={completeness} label="Profil" />
                <div>
                  <h3 className="panel-title">Profil-Fortschritt</h3>
                  <p className="sub" style={{ marginBottom: "0.6rem" }}>
                    Schließe dein Profil ab, um alles herauszuholen.
                  </p>
                  <ul className="feed" style={{ gap: "0.4rem" }}>
                    <ChecklistItem done label="Konto verbunden" />
                    <ChecklistItem done={!!activeId} label="Cape ausgewählt" />
                    <ChecklistItem done={settings.autoUpdate} label="Auto-Update aktiv" />
                  </ul>
                </div>
              </div>

              <div className="panel" style={{ margin: 0 }}>
                <h3 className="panel-title">Letzte Aktivität</h3>
                <p className="sub">Deine jüngsten Aktionen im Client.</p>
                <ul className="feed">
                  <FeedItem icon={I.play} title="Session gestartet" sub="Minecraft 26.1 · nativer Client" time="gerade" />
                  <FeedItem icon={I.cape} title={activeId ? "Cape geändert" : "Noch keine Cape"} sub="Cosmetics" time="vor 2 h" />
                  <FeedItem icon={I.download} title="Launcher aktualisiert" sub="v0.1.0 → aktuell" time="gestern" />
                </ul>
              </div>
            </div>
          </div>
        )}

        {tab === "cosmetics" && (
          <div className="dash__section panel" style={{ margin: 0 }}>
            <h3 className="panel-title">Cosmetics verwalten</h3>
            <p className="sub">
              Wähle deine Cape — sichtbar für andere DolphinClient-Spieler im Spiel.
            </p>
            <div className="cape-grid">
              <div
                className={`cape${activeId === null ? " active" : ""}`}
                role="button"
                tabIndex={0}
                onClick={() => setActive(null)}
                onKeyDown={(e) => e.key === "Enter" && setActive(null)}
              >
                <div className="cape__swatch" style={{ background: "repeating-linear-gradient(45deg,#1a2536,#1a2536 8px,#141d2e 8px,#141d2e 16px)" }} />
                <h3>Keine Cape</h3>
                <p>{activeId === null ? "Aktiv" : "Cape ausblenden"}</p>
              </div>
              {capes.map((cape) => (
                <div
                  key={cape.id}
                  className={`cape${activeId === cape.id ? " active" : ""}`}
                  role="button"
                  tabIndex={0}
                  onClick={() => setActive(cape.id)}
                  onKeyDown={(e) => e.key === "Enter" && setActive(cape.id)}
                >
                  <div className="cape__swatch" />
                  <h3>{cape.name}</h3>
                  <p>{activeId === cape.id ? "Aktiv" : "Auswählen"}</p>
                </div>
              ))}
            </div>
            {capes.length === 0 && (
              <p className="status err">
                Keine Cosmetics geladen — läuft das Backend? ({API_BASE})
              </p>
            )}
          </div>
        )}

        {tab === "stats" && (
          <div className="dash__section panel" style={{ margin: 0 }}>
            <h3 className="panel-title">Statistiken</h3>
            <p className="sub">Ein Überblick über deine Zeit mit DolphinClient.</p>
            <div className="stat-grid" style={{ marginBottom: "1.2rem" }}>
              <StatCard icon={I.clock} num={<Counter to={stats.hours} suffix=" h" />} lbl="Gesamtspielzeit" />
              <StatCard icon={I.bolt} num={<Counter to={stats.avgFps} />} lbl="Ø FPS" />
              <StatCard icon={I.grid} num={<Counter to={stats.blocks} group />} lbl="Blöcke gelaufen" />
            </div>
            <Bar label="Sodium (Rendering)" pct={92} />
            <Bar label="Lithium (Logik)" pct={78} />
            <Bar label="FerriteCore (RAM)" pct={64} />
            <Bar label="ImmediatelyFast" pct={55} />
          </div>
        )}

        {tab === "downloads" && (
          <div className="dash__section panel" style={{ margin: 0 }}>
            <h3 className="panel-title">Downloads &amp; Version</h3>
            <p className="sub">Dein installierter Client und der native Launcher.</p>
            <ul className="feed" style={{ marginBottom: "1.2rem" }}>
              <FeedItem icon={I.download} title="DolphinClient-Launcher" sub="Nativ (Rust) · Windows" time="v0.1.0" />
              <FeedItem icon={I.play} title="Minecraft" sub="Ziel-Version" time="26.1" />
              <FeedItem icon={I.bolt} title="Native Engine" sub="Rust · wgpu — kein Java/Fabric" time="26.1" />
            </ul>
            <div className="cta">
              <Link className="btn" href="/download">Zur Download-Seite</Link>
              <Link className="btn ghost" href="/download#changelog">Changelog ansehen</Link>
            </div>
          </div>
        )}

        {tab === "settings" && (
          <div className="dash__section panel" style={{ margin: 0 }}>
            <h3 className="panel-title">Launcher-Einstellungen</h3>
            <p className="sub">Werden lokal in deinem Browser gespeichert (Demo).</p>

            <div style={{ margin: "0.4rem 0 1.2rem" }}>
              <label style={{ display: "flex", justifyContent: "space-between", marginBottom: "0.5rem" }}>
                <b style={{ fontFamily: "var(--font-display)" }}>Zugewiesener RAM</b>
                <span className="tag">{settings.ram} GB</span>
              </label>
              <input
                type="range"
                min={2}
                max={16}
                step={1}
                value={settings.ram}
                onChange={(e) => saveSettings({ ...settings, ram: Number(e.target.value) })}
                aria-label="RAM in GB"
              />
            </div>

            <Toggle label="Auto-Update" desc="Launcher aktualisiert sich automatisch." on={settings.autoUpdate} onChange={(v) => saveSettings({ ...settings, autoUpdate: v })} />
            <Toggle label="FPS-Anzeige" desc="FPS-Modul im HUD anzeigen." on={settings.showFps} onChange={(v) => saveSettings({ ...settings, showFps: v })} />
            <Toggle label="Koordinaten" desc="XYZ + Blickrichtung im HUD." on={settings.showCoords} onChange={(v) => saveSettings({ ...settings, showCoords: v })} />
            <Toggle label="Keystrokes" desc="Tastenanzeige einblenden." on={settings.keystrokes} onChange={(v) => saveSettings({ ...settings, keystrokes: v })} />
            <Toggle label="Vollbild-Start" desc="Spiel direkt im Vollbild starten." on={settings.fullscreen} onChange={(v) => saveSettings({ ...settings, fullscreen: v })} />

            <button className="btn" style={{ marginTop: "0.6rem" }} onClick={() => notify("Einstellungen gespeichert.")}>
              Speichern
            </button>
          </div>
        )}
      </div>

      {toast && (
        <div className={`toast ${toast.kind}`}>
          <span className="dot" />
          {toast.msg}
        </div>
      )}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/*  Small presentational helpers                                       */
/* ------------------------------------------------------------------ */

function StatCard({ icon, num, lbl, trend }: { icon: JSX.Element; num: React.ReactNode; lbl: string; trend?: string }) {
  return (
    <div className="stat-card">
      <span className="ico">{icon}</span>
      {trend && <span className="trend">{trend}</span>}
      <div className="num">{num}</div>
      <div className="lbl">{lbl}</div>
    </div>
  );
}

function Ring({ value, label }: { value: number; label: string }) {
  const r = 52;
  const c = 2 * Math.PI * r;
  const offset = c * (1 - value / 100);
  return (
    <div className="ring" style={{ flex: "none" }}>
      <svg width="120" height="120" viewBox="0 0 120 120">
        <circle className="track" cx="60" cy="60" r={r} fill="none" strokeWidth="10" />
        <circle
          className="fill"
          cx="60"
          cy="60"
          r={r}
          fill="none"
          strokeWidth="10"
          strokeDasharray={c}
          strokeDashoffset={offset}
        />
      </svg>
      <div className="lbl">
        <b>{value}%</b>
        <span>{label}</span>
      </div>
    </div>
  );
}

function FeedItem({ icon, title, sub, time }: { icon: JSX.Element; title: string; sub: string; time: string }) {
  return (
    <li>
      <span className="fico">{icon}</span>
      <div>
        <b>{title}</b>
        <small>{sub}</small>
      </div>
      <time>{time}</time>
    </li>
  );
}

function ChecklistItem({ done, label }: { done?: boolean; label: string }) {
  return (
    <li style={{ opacity: done ? 1 : 0.6 }}>
      <span className="fico" style={{ background: done ? "rgba(126,240,212,0.14)" : undefined, color: done ? "var(--mint)" : "var(--muted)" }}>
        {I.check}
      </span>
      <b>{label}</b>
      {done && <time style={{ color: "var(--mint)" }}>erledigt</time>}
    </li>
  );
}

function Bar({ label, pct }: { label: string; pct: number }) {
  return (
    <div style={{ marginBottom: "0.9rem" }}>
      <div style={{ display: "flex", justifyContent: "space-between", marginBottom: "0.35rem", fontSize: "0.9rem" }}>
        <span>{label}</span>
        <span style={{ color: "var(--cyan)", fontVariantNumeric: "tabular-nums" }}>{pct}%</span>
      </div>
      <div className="hero__bar" style={{ margin: 0 }}>
        <i style={{ width: `${pct}%`, animation: "none" }} />
      </div>
    </div>
  );
}

function Toggle({ label, desc, on, onChange }: { label: string; desc: string; on: boolean; onChange: (v: boolean) => void }) {
  return (
    <div className="toggle-row">
      <div className="meta">
        <b>{label}</b>
        <small>{desc}</small>
      </div>
      <label className="switch">
        <input type="checkbox" checked={on} onChange={(e) => onChange(e.target.checked)} />
        <span className="slider" />
      </label>
    </div>
  );
}
