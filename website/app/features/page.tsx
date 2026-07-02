import type { Metadata } from "next";
import Link from "next/link";
import Reveal from "../components/Reveal";
import TiltCard from "../components/TiltCard";

export const metadata: Metadata = {
  title: "Features",
  description:
    "Performance, Module & HUD, ein nativer Rust-Launcher, Cosmetics und die Roadmap von DolphinClient für Minecraft 26.1.",
};

const check = (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
    <path d="m20 6-11 11-5-5" />
  </svg>
);

const MODULES = [
  { t: "FPS", d: "Bilder pro Sekunde, dezent im HUD." },
  { t: "Koordinaten", d: "XYZ inkl. Blickrichtung (N/O/S/W)." },
  { t: "Uhr", d: "Echtzeit-Uhr für lange Sessions." },
  { t: "Sitzungszeit", d: "Wie lange läuft die aktuelle Runde." },
  { t: "Geschwindigkeit", d: "Blöcke pro Sekunde in Bewegung." },
  { t: "Keystrokes", d: "WASD + Maus als Overlay." },
  { t: "Zoom", d: "Fernglas auf Taste C." },
  { t: "In-Game-Menü", d: "Rechte Umschalt öffnet die Modulverwaltung." },
];

const ROADMAP = [
  { tag: "Erledigt", done: true, title: "Client, Launcher, Backend, Website", body: "Monorepo mit lauffähigen Skeletten aller vier Komponenten." },
  { tag: "Erledigt", done: true, title: "26.1-Client mit HUD-Modulen", body: "Gegen echte 26.1-APIs kompiliert: FPS, Koordinaten, Uhr, Zoom, In-Game-Menü." },
  { tag: "Aktuell", done: true, title: "Nativer Rust-Launcher", body: "Blitzschnelles .exe: Microsoft-Login, Spielstart, Auto-Update — kein Electron mehr." },
  { tag: "Als Nächstes", done: false, title: "Cape-Rendering im Spiel", body: "Sichtbare Capes für andere DolphinClient-Nutzer, live aus der Cosmetics-API." },
  { tag: "Geplant", done: false, title: "Cosmetic-Store & Web-Login", body: "Microsoft-Login im Web, Store mit Zahlungen, verschiebbare HUD-Module." },
];

export default function FeaturesPage() {
  return (
    <main>
      <section className="hero" style={{ paddingBottom: "1rem" }}>
        <span className="hero__eyebrow">
          <span className="dot" /> Features · Überblick
        </span>
        <h1>
          Was <span className="glow">DolphinClient</span> kann
        </h1>
        <p className="tagline">
          Performance aus erstklassigen Mods, ein aufgeräumtes HUD, ein nativer
          Launcher und Cosmetics — ehrlich erklärt.
        </p>
      </section>

      {/* PERFORMANCE */}
      <Reveal as="section" variant="up" className="split" style={{ scrollMarginTop: "90px" }}>
        <div id="performance">
          <span className="eyebrow-sm">Performance</span>
          <h3>Flüssige FPS ohne Config-Gefummel</h3>
          <p>
            DolphinClient bündelt und konfiguriert die besten Open-Source-Performance-Mods.
            Du installierst nichts von Hand und stellst keine 40 Regler ein — es läuft einfach.
          </p>
          <ul>
            <li>{check}<span>Sodium für modernes, schnelles Rendering</span></li>
            <li>{check}<span>Lithium optimiert die Spiel-Logik (Ticks)</span></li>
            <li>{check}<span>FerriteCore senkt den RAM-Verbrauch</span></li>
            <li>{check}<span>ImmediatelyFast beschleunigt UI/Text-Rendering</span></li>
          </ul>
        </div>
        <div className="split__media">
          <div className="hud">
            <div className="hud__row"><span className="k">FPS Vanilla</span><span className="v">118</span></div>
            <div className="hud__row"><span className="k">FPS DolphinClient</span><span className="v good">324</span></div>
            <div className="hud__row"><span className="k">RAM</span><span className="v good">-28 %</span></div>
          </div>
        </div>
      </Reveal>

      {/* MODULES */}
      <Reveal as="section" className="section-head" style={{ scrollMarginTop: "90px" }}>
        <span className="eyebrow-sm" id="modules">Module &amp; HUD</span>
        <h2 className="section-title">Nur an, was du brauchst</h2>
        <p className="section-sub">
          Jedes Modul lässt sich einzeln schalten. Deaktivierte Module durchlaufen
          keinen Code-Pfad — sie kosten keine Leistung.
        </p>
      </Reveal>
      <section className="card-grid">
        {MODULES.map((m, i) => (
          <Reveal key={m.t} variant="up" delay={i * 50}>
            <TiltCard className="feature spotlight">
              <h3>{m.t}</h3>
              <p>{m.d}</p>
            </TiltCard>
          </Reveal>
        ))}
      </section>

      {/* LAUNCHER */}
      <Reveal as="section" variant="up" className="split reverse" style={{ scrollMarginTop: "90px" }}>
        <div className="split__media">
          <div className="hud">
            <div className="hud__row"><span className="k">Sprache</span><span className="v good">Rust 🦀</span></div>
            <div className="hud__row"><span className="k">Framework</span><span className="v">egui (nativ)</span></div>
            <div className="hud__row"><span className="k">Binärgröße</span><span className="v">~12 MB</span></div>
            <div className="hud__row"><span className="k">Kaltstart</span><span className="v good">&lt; 0.5 s</span></div>
          </div>
        </div>
        <div id="launcher">
          <span className="eyebrow-sm">Nativer Launcher</span>
          <h3>Eine echte Windows-App — in Rust</h3>
          <p>
            Der Launcher ist von Grund auf in Rust neu geschrieben und rendert nativ.
            Kein mitgeliefertes Chromium, kein Node — nur ein winziges, schnelles Programm.
          </p>
          <ul>
            <li>{check}<span>Microsoft-Device-Code-Login (kein eingebetteter Browser nötig)</span></li>
            <li>{check}<span>Lädt 26.1 + Libraries + Assets direkt von Mojang</span></li>
            <li>{check}<span>Installiert Fabric + DolphinClient-Mod automatisch</span></li>
            <li>{check}<span>Refresh-Token sicher in der Windows-Keychain (DPAPI)</span></li>
            <li>{check}<span>Auto-Update gegen den Release-Feed</span></li>
          </ul>
          <div className="cta">
            <Link className="btn" href="/download">Launcher herunterladen</Link>
          </div>
        </div>
      </Reveal>

      {/* ROADMAP */}
      <Reveal as="section" className="section-head" style={{ scrollMarginTop: "90px" }}>
        <span className="eyebrow-sm" id="roadmap">Roadmap</span>
        <h2 className="section-title">Wohin die Reise geht</h2>
        <p className="section-sub">Ehrlich: Ein Client auf Lunar-Niveau ist Mann-Jahre Arbeit. Wir bauen Schritt für Schritt.</p>
      </Reveal>
      <section className="changelog" style={{ maxWidth: 820, marginInline: "auto" }}>
        {ROADMAP.map((r, i) => (
          <Reveal key={r.title} variant="left" delay={i * 60}>
            <div className="change" style={{ opacity: r.done ? 1 : 0.9 }}>
              <h4>
                {r.title} <span className={`pill ${r.done ? "on" : ""}`}>{r.tag}</span>
              </h4>
              <p style={{ margin: 0, color: "var(--muted)" }}>{r.body}</p>
            </div>
          </Reveal>
        ))}
      </section>

      {/* CTA */}
      <Reveal as="section" variant="zoom" className="cta-band">
        <h2>Selbst ausprobieren</h2>
        <p>Lade den nativen Launcher und spiele Minecraft 26.1 mit mehr FPS.</p>
        <div className="cta">
          <Link className="btn lg" href="/download">Herunterladen</Link>
          <Link className="btn ghost lg" href="/dashboard">Zum Dashboard</Link>
        </div>
      </Reveal>
    </main>
  );
}
