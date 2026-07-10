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
  { t: "FPS", d: "Bilder pro Sekunde im F3-Overlay." },
  { t: "Koordinaten", d: "XYZ inkl. Yaw/Pitch der Blickrichtung." },
  { t: "Leben & Hunger", d: "Live-Werte direkt aus dem Server-State." },
  { t: "Geladene Chunks", d: "Gezeichnete/gesamte Sections + Mesh-Queue." },
  { t: "Render-Distanz", d: "2–32 Chunks, im Menü live einstellbar." },
  { t: "Max Framerate", d: "Eigenes FPS-Limit — oder komplett uncapped." },
  { t: "FOV & Helligkeit", d: "Sichtfeld und Gamma frei justierbar." },
  { t: "GUI-Skalierung", d: "Auto oder 1–4×, genau wie in Vanilla." },
  { t: "Lautstärke", d: "Master + 9 Kategorien einzeln regelbar — wie im Vanilla-Sound-Menü." },
];

const ROADMAP = [
  { tag: "Erledigt", done: true, title: "Client, Launcher & Website", body: "Drei schlanke Komponenten: nativer Client und Launcher in Rust, Website mit Live-Dashboard." },
  { tag: "Erledigt", done: true, title: "Volles Vanilla-Menü im Client", body: "Titelbildschirm, Optionen mit Video-/Steuerungs-/Chat-/Sound-Untermenüs, Esc-Pause — plus F3-Debug-Overlay." },
  { tag: "Erledigt", done: true, title: "Uncapped FPS & Live-Optionen", body: "VSync abschaltbar, FPS-Limit, Render-Distanz, GUI-Skalierung, Helligkeit — alles im Spiel einstellbar." },
  { tag: "Erledigt", done: true, title: "Echter Vanilla-Sound", body: "Originale Mojang-Sounds (Blöcke, Schritte, Mobs, Musik), on-demand geladen — mit Lautstärke-Reglern pro Kategorie." },
  { tag: "Erledigt", done: true, title: "Selbstaktualisierender Client", body: "Der Launcher prüft die Client-Signatur bei jedem Start und lädt automatisch die neueste Version — nie wieder eine veraltete Build." },
  { tag: "Aktuell", done: true, title: "Neuer Launcher & Live-Dashboard", body: "Neu gestalteter Launcher mit auto-speichernden Einstellungen; Web-Dashboard, das sich live mit dem laufenden Launcher verbindet." },
  { tag: "Als Nächstes", done: false, title: "Cape-Rendering im Spiel", body: "Die im Launcher gewählte Cape auch im Spiel sichtbar machen — für andere DolphinClient-Nutzer." },
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
          Eine native Engine in Rust, ein aufgeräumtes HUD, ein nativer
          Launcher und Cosmetics — ehrlich erklärt.
        </p>
      </section>

      {/* PERFORMANCE */}
      <Reveal as="section" variant="up" className="split" style={{ scrollMarginTop: "90px" }}>
        <div id="performance">
          <span className="eyebrow-sm">Performance</span>
          <h3>Flüssige FPS ohne Config-Gefummel</h3>
          <p>
            DolphinClient ist kein Modpack — es rendert Minecraft 26.1 selbst in
            Rust (wgpu). Kein Java, kein Fabric: hohe FPS und schneller Start sind
            eingebaut, du stellst keine 40 Regler ein.
          </p>
          <ul>
            <li>{check}<span>Nativer wgpu-Renderer (Vulkan / Metal / DX12)</span></li>
            <li>{check}<span>Chunks werden parallel gemesht (rayon) — keine Ruckler</span></li>
            <li>{check}<span>Kein JVM-Warmup, wenig RAM — läuft auch auf schwachen PCs</span></li>
            <li>{check}<span>Assets einmal in ~0,4 s gebacken, dann sofort spielbereit</span></li>
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
        <span className="eyebrow-sm" id="modules">HUD &amp; Optionen</span>
        <h2 className="section-title">Alles, was zählt — im Blick und einstellbar</h2>
        <p className="section-sub">
          Das F3-Debug-Overlay zeigt die Live-Werte, und ein volles
          Vanilla-Optionsmenü stellt Video, Steuerung und Chat ein.
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
            <div className="hud__row"><span className="k">Binärgröße</span><span className="v">~6 MB</span></div>
            <div className="hud__row"><span className="k">Kaltstart</span><span className="v good">&lt; 0.5 s</span></div>
          </div>
        </div>
        <div id="launcher">
          <span className="eyebrow-sm">Nativer Launcher</span>
          <h3>Eine echte Windows-App — mit mehreren Accounts</h3>
          <p>
            Der Launcher ist von Grund auf in Rust neu geschrieben und rendert nativ.
            Kein mitgeliefertes Chromium, kein Node — nur ein winziges, schnelles Programm,
            das beliebig viele Konten verwaltet.
          </p>
          <ul>
            <li>{check}<span>Mehrere Microsoft-Konten hinzufügen, wechseln &amp; entfernen</span></li>
            <li>{check}<span>Auto-Import bereits angemeldeter Konten aus Vanilla- &amp; Lunar-Launcher</span></li>
            <li>{check}<span>Lädt Original-Dateien von Mojang und den nativen Client automatisch</span></li>
            <li>{check}<span>Prüft die Client-Signatur (SHA-256) und hält ihn stets auf der neuesten Version</span></li>
            <li>{check}<span>Refresh-Token sicher in der Windows-Keychain (DPAPI)</span></li>
            <li>{check}<span>Auto-Update von Launcher und Client gegen den Release-Feed</span></li>
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
