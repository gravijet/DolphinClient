import type { Metadata } from "next";
import Link from "next/link";
import Reveal from "../components/Reveal";

export const metadata: Metadata = {
  title: "Technik",
  description:
    "Wie DolphinClient gebaut ist: die native Rust-Engine, das F3-HUD und die Spiel-Optionen, der native Launcher und die Roadmap für Minecraft 26.1.",
};

const MODULES = [
  { t: "FPS", d: "Bilder pro Sekunde, live im F3-Overlay." },
  { t: "Koordinaten", d: "XYZ inklusive Yaw/Pitch der Blickrichtung." },
  { t: "Leben & Hunger", d: "Werte direkt aus dem Server-State." },
  { t: "Geladene Chunks", d: "Gezeichnete und gesamte Sections plus Mesh-Queue." },
  { t: "Render-Distanz", d: "2 bis 32 Chunks, im Menü live einstellbar." },
  { t: "Max. Framerate", d: "Eigenes FPS-Limit — oder komplett uncapped." },
  { t: "FoV & Helligkeit", d: "Sichtfeld und Gamma frei justierbar." },
  { t: "GUI-Skalierung", d: "Auto oder 1× bis 4×, wie in Vanilla." },
  { t: "Lautstärke", d: "Master plus neun Kategorien einzeln geregelt." },
];

const LAUNCHER = [
  { t: "Server-Liste", d: "Beliebig viele Server speichern, einen Standard festlegen und mit einem Klick beitreten." },
  { t: "Spiel-Schnelleinstellungen", d: "Render-Distanz, FPS-Limit, FoV, Helligkeit, VSync, GUI-Skalierung, Grafik & Discord — vorab im Launcher." },
  { t: "Skin-Vorschau", d: "Dein Ganzkörper-Skin im Profil, gerendert aus dem Minecraft-Konto." },
  { t: "Spielzeit-Historie", d: "Eine Sparkline der letzten Sitzungen, dazu Gesamt- und Durchschnittswerte." },
  { t: "Multi-Account", d: "Beliebig viele Microsoft-Konten hinzufügen, wechseln, entfernen — plus Import aus Vanilla & Lunar." },
  { t: "Auto-Update", d: "Launcher und Client halten sich per SHA-256-Abgleich gegen den Release-Feed aktuell." },
];

const ROADMAP = [
  { tag: "Erledigt", done: true, title: "Client, Launcher & Website", body: "Drei schlanke Teile: nativer Client und Launcher in Rust, dazu diese Website mit Live-Dashboard." },
  { tag: "Erledigt", done: true, title: "Volles Vanilla-Menü", body: "Titelbildschirm, Optionen mit Video-, Steuerungs-, Chat- und Sound-Untermenüs, Esc-Pause — plus F3-Overlay." },
  { tag: "Erledigt", done: true, title: "Uncapped FPS & Live-Optionen", body: "VSync abschaltbar, FPS-Limit, Render-Distanz, GUI-Skalierung, Helligkeit — alles im Spiel einstellbar." },
  { tag: "Erledigt", done: true, title: "Echter Vanilla-Sound", body: "Originale Mojang-Sounds, on-demand geladen, mit Reglern pro Kategorie." },
  { tag: "Erledigt", done: true, title: "Selbstaktualisierender Client", body: "Der Launcher prüft die Client-Signatur bei jedem Start und zieht die neueste Version." },
  { tag: "Erledigt", done: true, title: "Server-Liste & Quick-Settings", body: "Server im Launcher speichern und beitreten, Spiel-Optionen vorab setzen, Skin-Vorschau & Historie." },
  { tag: "Als Nächstes", done: false, title: "Cape-Rendering im Spiel", body: "Die im Launcher gewählte Cape auch in der Welt sichtbar machen — für andere DolphinClient-Spieler." },
];

export default function FeaturesPage() {
  return (
    <main>
      <section className="hero" style={{ paddingBottom: "1rem" }}>
        <span className="kicker">Technik · Aufbau</span>
        <h1>
          Wie <span className="accent">DolphinClient</span>
          <br />
          gebaut ist.
        </h1>
        <p className="hero__lede">
          Kein Marketing-Nebel — der ehrliche Aufbau: die native Engine, das HUD
          und die Optionen, der Launcher und was als Nächstes kommt.
        </p>
      </section>

      {/* ENGINE */}
      <Reveal as="section" variant="up" className="split" style={{ scrollMarginTop: "90px", marginTop: "2.5rem" }}>
        <div id="engine">
          <span className="kicker">Engine</span>
          <h3>Flüssige Frames ohne Config-Gefummel</h3>
          <p>
            DolphinClient ist kein Modpack — es rendert Minecraft 26.1 selbst in
            Rust über wgpu. Kein Java, kein Fabric: hohe FPS und schneller Start
            sind eine Eigenschaft der Architektur, kein Preset, das du finden musst.
          </p>
          <ul>
            <li><span className="mk">GPU</span><span>wgpu-Renderer über Vulkan, Metal oder DirectX 12</span></li>
            <li><span className="mk">MESH</span><span>Chunks parallel gemesht (rayon) — keine Ruckler</span></li>
            <li><span className="mk">LEAN</span><span>kein JVM-Warmup, wenig RAM — auch auf schwachen PCs</span></li>
            <li><span className="mk">CACHE</span><span>Assets einmal in ~0,4 s gebacken, dann sofort startklar</span></li>
          </ul>
        </div>
        <div className="split__media">
          <div className="spec">
            <div className="spec__head"><span className="dot" /> benchmark<span className="spec__tag">richtwert</span></div>
            <div className="spec__body">
              <div className="spec__row"><span className="spec__k">java-vanilla</span><span className="spec__leader" /><span className="spec__v">118 fps</span></div>
              <div className="spec__row"><span className="spec__k">dolphinclient</span><span className="spec__leader" /><span className="spec__v good">324 fps</span></div>
              <div className="spec__row"><span className="spec__k">ram</span><span className="spec__leader" /><span className="spec__v good">−28 %</span></div>
              <div className="spec__row"><span className="spec__k">kaltstart</span><span className="spec__leader" /><span className="spec__v good">0,42 s</span></div>
            </div>
            <div className="spec__foot">abhängig von hardware &amp; szene</div>
          </div>
        </div>
      </Reveal>

      {/* HUD */}
      <Reveal as="section" className="sec-head" style={{ scrollMarginTop: "90px" }}>
        <span className="sec-head__idx" id="hud">[ HUD &amp; Optionen ]</span>
        <span className="kicker">Im Blick &amp; einstellbar</span>
        <h2 className="sec-title">Alles, was zählt — sichtbar und regelbar</h2>
        <p className="sec-lede">
          Das F3-Overlay zeigt die Live-Werte, ein volles Vanilla-Optionsmenü
          stellt Video, Steuerung, Chat und Sound ein.
        </p>
      </Reveal>
      <section className="sheet">
        <div className="sheet__grid cols-3">
          {MODULES.map((m, i) => (
            <Reveal key={m.t} variant="up" delay={(i % 3) * 60}>
              <div className="cell">
                <span className="cell__idx">{String(i + 1).padStart(2, "0")}</span>
                <h3 style={{ marginTop: "0.9rem" }}>{m.t}</h3>
                <p>{m.d}</p>
              </div>
            </Reveal>
          ))}
        </div>
      </section>

      {/* LAUNCHER */}
      <Reveal as="section" variant="up" className="split reverse" style={{ scrollMarginTop: "90px" }}>
        <div className="split__media">
          <div className="spec">
            <div className="spec__head"><span className="dot" /> launcher.build<span className="spec__tag">rust</span></div>
            <div className="spec__body">
              <div className="spec__row"><span className="spec__k">framework</span><span className="spec__leader" /><span className="spec__v aqua">egui · nativ</span></div>
              <div className="spec__row"><span className="spec__k">binary</span><span className="spec__leader" /><span className="spec__v">~6 MB</span></div>
              <div className="spec__row"><span className="spec__k">kaltstart</span><span className="spec__leader" /><span className="spec__v good">&lt; 0,5 s</span></div>
              <div className="spec__row"><span className="spec__k">konten</span><span className="spec__leader" /><span className="spec__v">unbegrenzt</span></div>
            </div>
          </div>
        </div>
        <div id="launcher">
          <span className="kicker">Der Launcher</span>
          <h3>Eine echte App — mit mehreren Konten</h3>
          <p>
            Der Launcher ist von Grund auf in Rust geschrieben und rendert nativ.
            Kein mitgeliefertes Chromium, kein Node — nur ein winziges, schnelles
            Programm, das beliebig viele Konten verwaltet.
          </p>
          <ul>
            <li><span className="mk">MULTI</span><span>Microsoft-Konten hinzufügen, wechseln &amp; entfernen</span></li>
            <li><span className="mk">IMPORT</span><span>bereits angemeldete Konten aus Vanilla &amp; Lunar</span></li>
            <li><span className="mk">SYNC</span><span>lädt Original-Assets von Mojang und den Client automatisch</span></li>
            <li><span className="mk">HASH</span><span>prüft die SHA-256-Signatur und hält den Client aktuell</span></li>
            <li><span className="mk">SAFE</span><span>Refresh-Tokens in der OS-Keychain (DPAPI)</span></li>
          </ul>
          <div className="cta">
            <Link className="btn" href="/download">Launcher beziehen</Link>
          </div>
        </div>
      </Reveal>

      {/* LAUNCHER FEATURES */}
      <Reveal as="section" className="sec-head">
        <span className="sec-head__idx">[ Launcher-Funktionen ]</span>
        <span className="kicker">Mehr als ein Startknopf</span>
        <h2 className="sec-title">Konten, Server und Optionen an einem Ort</h2>
      </Reveal>
      <section className="sheet">
        <div className="sheet__grid cols-3">
          {LAUNCHER.map((m, i) => (
            <Reveal key={m.t} variant="up" delay={(i % 3) * 60}>
              <div className="cell">
                <span className="cell__idx">{String(i + 1).padStart(2, "0")}</span>
                <h3 style={{ marginTop: "0.9rem" }}>{m.t}</h3>
                <p>{m.d}</p>
              </div>
            </Reveal>
          ))}
        </div>
      </section>

      {/* ROADMAP */}
      <Reveal as="section" className="sec-head" style={{ scrollMarginTop: "90px" }}>
        <span className="sec-head__idx" id="roadmap">[ Roadmap ]</span>
        <span className="kicker">Wohin die Reise geht</span>
        <h2 className="sec-title">Ehrlich: Schritt für Schritt</h2>
        <p className="sec-lede">
          Ein Client auf dem Niveau der großen Namen ist Mann-Jahre Arbeit. Wir
          bauen sichtbar und in Etappen — hier steht, was fertig ist und was folgt.
        </p>
      </Reveal>
      <section className="changelog">
        {ROADMAP.map((r) => (
          <Reveal key={r.title} variant="left">
            <div className={`change${r.done ? " is-current" : ""}`}>
              <div className="change__head">
                <span className="change__v">{r.title}</span>
                <span className={`pill ${r.done ? "on" : "aqua"}`}>{r.tag}</span>
              </div>
              <p style={{ margin: "0.7rem 0 0", color: "var(--muted)", fontSize: "0.95rem" }}>{r.body}</p>
            </div>
          </Reveal>
        ))}
      </section>

      {/* CTA */}
      <Reveal as="section" variant="zoom" className="cta-band">
        <span className="kicker">Selbst nachprüfen</span>
        <h2>Zahlen sind billig. Probier es aus.</h2>
        <p>Lade den nativen Launcher und miss die FPS auf deiner Hardware selbst nach.</p>
        <div className="cta">
          <Link className="btn lg" href="/download">Beziehen</Link>
          <Link className="btn ghost lg" href="/dashboard">Zum Dashboard</Link>
        </div>
      </Reveal>
    </main>
  );
}
