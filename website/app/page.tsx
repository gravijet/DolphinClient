import Link from "next/link";
import Reveal from "./components/Reveal";
import Counter from "./components/Counter";
import TiltCard from "./components/TiltCard";

const I = {
  bolt: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M13 2 3 14h9l-1 8 10-12h-9l1-8Z" />
    </svg>
  ),
  toggle: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="2" y="7" width="20" height="10" rx="5" />
      <circle cx="9" cy="12" r="3" fill="currentColor" stroke="none" />
    </svg>
  ),
  cape: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M6 3v10c0 3 2.7 5 6 5s6-2 6-5V3" />
      <path d="M6 3c0 2 2.7 3 6 3s6-1 6-3" />
    </svg>
  ),
  rocket: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M5 15c-1.5 1.5-2 5-2 5s3.5-.5 5-2c.8-.8.8-2.2 0-3s-2.2-.8-3 0Z" />
      <path d="M9 12a12 12 0 0 1 8-8c2 0 3 1 3 3a12 12 0 0 1-8 8l-3-3Z" />
      <circle cx="15" cy="9" r="1.6" />
    </svg>
  ),
  refresh: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M21 12a9 9 0 1 1-3-6.7" />
      <path d="M21 3v5h-5" />
    </svg>
  ),
  chip: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="6" y="6" width="12" height="12" rx="2" />
      <path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3" />
    </svg>
  ),
  check: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
      <path d="m20 6-11 11-5-5" />
    </svg>
  ),
};

const FEATURES = [
  { icon: I.bolt, cls: "", title: "Performance out of the box", body: "Sodium, Lithium & Co. sind vorkonfiguriert — flüssige FPS ohne Gefummel an Config-Dateien." },
  { icon: I.toggle, cls: "violet", title: "Module an/aus", body: "Aktiviere nur, was du brauchst. Deaktivierte Module durchlaufen keinen Code-Pfad — null Overhead." },
  { icon: I.cape, cls: "pink", title: "Cosmetics", body: "Capes und mehr — sichtbar für andere DolphinClient-Nutzer direkt im Spiel." },
  { icon: I.rocket, cls: "", title: "Nativer Launcher", body: "In Rust geschrieben: ein winziges, blitzschnelles .exe. Kein Electron, kein Browser im Hintergrund." },
  { icon: I.refresh, cls: "gold", title: "Auto-Update", body: "Login, Fabric-Setup und Updates erledigt der Launcher automatisch — ein Klick, und es läuft." },
  { icon: I.chip, cls: "violet", title: "Für 26.1 gebaut", body: "Unobfuskiertes Minecraft 26.1 auf Java 25, Fabric 0.19.3 — verifiziert gegen echte APIs kompiliert." },
];

const STEPS = [
  { title: "Launcher laden", body: "Lade das native DolphinClient-Setup für dein System herunter und installiere es in Sekunden." },
  { title: "Mit Microsoft anmelden", body: "Sichere Anmeldung über den offiziellen Microsoft-Login. Kein Passwort verlässt je deinen Rechner." },
  { title: "Ein Klick", body: "Der Launcher lädt 26.1 von Mojang, richtet Fabric + Mods ein und hält alles aktuell." },
  { title: "Spielen", body: "Starte mit mehr FPS, HUD-Modulen und deinen Cosmetics — sofort spielbereit." },
];

const MODS = ["Sodium", "Lithium", "Iris", "FerriteCore", "ImmediatelyFast", "Fabric API", "DolphinClient HUD"];

const FAQ = [
  { q: "Ist DolphinClient kostenlos?", a: "Ja. Der Client, der Launcher und die Basis-Cosmetics sind kostenlos. Du brauchst nur ein gekauftes Minecraft-Konto." },
  { q: "Brauche ich ein Minecraft-Konto?", a: "Ja — ein gültiges Microsoft/Minecraft-Konto. Der Launcher lädt die Original-Spieldateien direkt von Mojang. Keine Cracked-Accounts." },
  { q: "Warum ist der Launcher in Rust?", a: "Ein nativer Rust-Launcher startet quasi sofort, braucht kaum RAM und ist ein einziges kleines .exe — kein mitgeliefertes Chromium wie bei Electron." },
];

export default function HomePage() {
  return (
    <main>
      {/* ---------- HERO ---------- */}
      <section className="hero">
        <div className="hero__grid">
          <div>
            <span className="hero__eyebrow">
              <span className="dot" /> Performance · Minecraft 26.1
            </span>
            <h1>
              Mehr FPS, weniger Aufwand.
              <br />
              <span className="glow">DolphinClient.</span>
            </h1>
            <p className="tagline">
              Vorkonfigurierte Performance-Mods, Cosmetics und ein blitzschneller
              nativer Launcher mit Microsoft-Login — für ein flüssiges Minecraft 26.1.
            </p>
            <p className="honest">
              Ehrlich gesagt: Die FPS kommen aus erstklassigen Open-Source-Mods. Wir
              bündeln sie bequem an einem Ort und bringen Cosmetics &amp; Community
              dazu — ein Klick, und es läuft.
            </p>

            <div className="cta">
              <Link className="btn lg" href="/download">
                Herunterladen
              </Link>
              <Link className="btn ghost lg" href="/dashboard">
                Dashboard öffnen
              </Link>
            </div>

            <div className="stats">
              <div className="stat">
                <b>26.1</b>
                <span>Minecraft-Ziel</span>
              </div>
              <div className="stat">
                <b>1</b>
                <span>Klick zum Spielen</span>
              </div>
              <div className="stat">
                <b>Auto</b>
                <span>Updates &amp; Fabric-Setup</span>
              </div>
            </div>
          </div>

          {/* launcher preview mockup */}
          <Reveal variant="left" delay={120}>
            <div className="hero__preview">
              <div className="hero__preview-bar">
                <i /><i /><i />
                <span>DolphinClient · nativ</span>
              </div>
              <div className="hero__preview-body">
                <div className="hero__player">
                  <div className="hero__avatar" />
                  <div>
                    <b>Steve</b>
                    <small>● Angemeldet über Microsoft</small>
                  </div>
                </div>
                <div className="hud">
                  <div className="hud__row"><span className="k">FPS</span><span className="v good">324</span></div>
                  <div className="hud__row"><span className="k">Version</span><span className="v">26.1 · Fabric</span></div>
                  <div className="hud__row"><span className="k">Module</span><span className="v">7 aktiv</span></div>
                </div>
                <div className="hero__bar"><i /></div>
                <div className="hero__preview-play">Spielen (26.1)</div>
              </div>
            </div>
          </Reveal>
        </div>

        {/* mod marquee */}
        <div className="marquee">
          <div className="marquee__track">
            {[...MODS, ...MODS].map((m, i) => (
              <span key={i} className="marquee__item">
                <b>◆</b> {m}
              </span>
            ))}
          </div>
        </div>
      </section>

      {/* ---------- FEATURES ---------- */}
      <Reveal as="section" className="section-head">
        <span className="eyebrow-sm">Warum DolphinClient?</span>
        <h2 className="section-title">Alles, was ein großer Client bietet</h2>
        <p className="section-sub">
          Kein FPS-Wunder aus dem Nichts — sondern die besten Mods, bequem
          gebündelt, plus die Extras eines Premium-Clients.
        </p>
      </Reveal>

      <section className="features">
        {FEATURES.map((f, i) => (
          <Reveal key={f.title} variant="up" delay={i * 70}>
            <TiltCard className="feature spotlight">
              <span className={`puck ${f.cls}`}>{f.icon}</span>
              <h3>{f.title}</h3>
              <p>{f.body}</p>
            </TiltCard>
          </Reveal>
        ))}
      </section>

      {/* ---------- HOW IT WORKS ---------- */}
      <Reveal as="section" className="section-head">
        <span className="eyebrow-sm">In 4 Schritten</span>
        <h2 className="section-title">Von Download bis Spielstart</h2>
        <p className="section-sub">Kein Fummeln mit Java, Fabric oder Configs. Der Launcher macht alles.</p>
      </Reveal>
      <section className="steps">
        {STEPS.map((s, i) => (
          <Reveal key={s.title} variant="up" delay={i * 90}>
            <div className="step">
              <span className="step__n" />
              <h3>{s.title}</h3>
              <p>{s.body}</p>
              {i < STEPS.length - 1 && <span className="step__line" />}
            </div>
          </Reveal>
        ))}
      </section>

      {/* ---------- STATS BAND ---------- */}
      <Reveal as="section" variant="zoom" className="band">
        <div className="band__grid">
          <div>
            <div className="band__num"><Counter to={3} suffix="×" /></div>
            <div className="band__label">mehr FPS ggü. Vanilla*</div>
          </div>
          <div>
            <div className="band__num"><Counter to={0.4} decimals={1} suffix=" s" /></div>
            <div className="band__label">Launcher-Kaltstart</div>
          </div>
          <div>
            <div className="band__num"><Counter to={12} suffix=" MB" /></div>
            <div className="band__label">Launcher-Größe</div>
          </div>
          <div>
            <div className="band__num"><Counter to={7} /></div>
            <div className="band__label">HUD-Module</div>
          </div>
        </div>
      </Reveal>

      {/* ---------- SPLIT: PERFORMANCE ---------- */}
      <Reveal as="section" variant="up" className="split" style={{}}>
        <div>
          <span className="eyebrow-sm">Performance & HUD</span>
          <h3>FPS, die man sieht — und ein HUD, das hilft</h3>
          <p>
            Vorkonfigurierte Renderer- und Logik-Mods holen aus jedem Frame das
            Maximum. Das DolphinClient-HUD zeigt dir genau, was zählt — dezent,
            frei anordbar und ohne Leistung zu kosten.
          </p>
          <ul>
            <li>{I.check}<span>FPS, Koordinaten &amp; Blickrichtung, Uhr, Sitzungszeit, Geschwindigkeit</span></li>
            <li>{I.check}<span>Module einzeln an/aus — deaktiviert = null Overhead</span></li>
            <li>{I.check}<span>In-Game-Menü (Rechte Umschalt) &amp; Zoom (C)</span></li>
          </ul>
        </div>
        <div className="split__media">
          <div className="hud">
            <div className="hud__row"><span className="k">FPS</span><span className="v good">312</span></div>
            <div className="hud__row"><span className="k">XYZ</span><span className="v">128 / 71 / -42</span></div>
            <div className="hud__row"><span className="k">Blick</span><span className="v">Nord-Ost</span></div>
            <div className="hud__row"><span className="k">Speed</span><span className="v">5.6 b/s</span></div>
            <div className="hud__row"><span className="k">Session</span><span className="v">01:24:07</span></div>
          </div>
        </div>
      </Reveal>

      {/* ---------- SPLIT: LAUNCHER (Rust) ---------- */}
      <Reveal as="section" variant="up" className="split reverse">
        <div className="split__media">
          <div className="hud">
            <div className="hud__row"><span className="k">Sprache</span><span className="v good">Rust 🦀</span></div>
            <div className="hud__row"><span className="k">Binärgröße</span><span className="v">~12 MB</span></div>
            <div className="hud__row"><span className="k">RAM (idle)</span><span className="v">~30 MB</span></div>
            <div className="hud__row"><span className="k">Start</span><span className="v good">&lt; 0.5 s</span></div>
          </div>
        </div>
        <div>
          <span className="eyebrow-sm">Nativer Launcher</span>
          <h3>Ein echtes Windows-Programm — kein Browser im Hintergrund</h3>
          <p>
            Der neue Launcher ist in Rust geschrieben und rendert nativ: ein
            einziges, winziges .exe, das quasi sofort startet und kaum Speicher
            braucht. Microsoft-Login, Spielstart und Auto-Update — alles direkt im
            Programm.
          </p>
          <ul>
            <li>{I.check}<span>Nativer Code statt Electron/Chromium</span></li>
            <li>{I.check}<span>Sichere Token-Ablage über die Windows-Keychain (DPAPI)</span></li>
            <li>{I.check}<span>Lädt Original-Dateien von Mojang, installiert Fabric + Mods automatisch</span></li>
          </ul>
          <div className="cta">
            <Link className="btn" href="/download">Launcher holen</Link>
            <Link className="btn ghost" href="/features#launcher">Mehr erfahren</Link>
          </div>
        </div>
      </Reveal>

      {/* ---------- FAQ ---------- */}
      <Reveal as="section" className="section-head">
        <span className="eyebrow-sm">Häufige Fragen</span>
        <h2 className="section-title">Kurz &amp; ehrlich beantwortet</h2>
      </Reveal>
      <section className="faq">
        {FAQ.map((f, i) => (
          <Reveal key={f.q} variant="fade" delay={i * 60}>
            <details open={i === 0}>
              <summary>{f.q}</summary>
              <p>{f.a}</p>
            </details>
          </Reveal>
        ))}
      </section>

      {/* ---------- CTA BAND ---------- */}
      <Reveal as="section" variant="zoom" className="cta-band">
        <h2>Bereit für flüssiges 26.1?</h2>
        <p>Lade den nativen Launcher, melde dich mit Microsoft an und spiele in unter einer Minute.</p>
        <div className="cta">
          <Link className="btn lg" href="/download">Jetzt herunterladen</Link>
          <Link className="btn ghost lg" href="/features">Alle Features ansehen</Link>
        </div>
      </Reveal>

      <p className="honest" style={{ marginTop: "2rem", textAlign: "center" }}>
        <small>* Grober Richtwert je nach Hardware und Szene. Die Performance stammt aus Open-Source-Mods (Sodium &amp; Co.).</small>
      </p>
    </main>
  );
}
