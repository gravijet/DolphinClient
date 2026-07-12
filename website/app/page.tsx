import Link from "next/link";
import Reveal from "./components/Reveal";
import Compare from "./components/Compare";
import Logo from "./components/Logo";

/* -------- soft line icons -------- */
const s = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.7,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};
const I = {
  bolt: (
    <svg viewBox="0 0 24 24" {...s}>
      <path d="M13 2 4 14h7l-1 8 9-12h-7l1-8Z" />
    </svg>
  ),
  timer: (
    <svg viewBox="0 0 24 24" {...s}>
      <circle cx="12" cy="13" r="8" />
      <path d="M12 13V9M9 2h6M18 6l1.5-1.5" />
    </svg>
  ),
  feather: (
    <svg viewBox="0 0 24 24" {...s}>
      <path d="M20 4c-6 0-11 4-13 10l-3 6M20 4c0 6-4 11-10 13M20 4 8 16M13 9h4M9 13h4" />
    </svg>
  ),
  layers: (
    <svg viewBox="0 0 24 24" {...s}>
      <path d="m12 3 9 5-9 5-9-5 9-5Z" />
      <path d="m3 13 9 5 9-5" />
    </svg>
  ),
  check: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
      <path d="m20 6-11 11-5-5" />
    </svg>
  ),
};

const BENEFITS = [
  {
    icon: I.bolt,
    title: "Mehr FPS, sofort spürbar",
    body: "Dieselbe Welt, dieselben Server — nur deutlich flüssiger. Bewegungen wirken direkter, Kämpfe fairer, alles reagiert schneller auf dich.",
  },
  {
    icon: I.timer,
    title: "In Sekunden startklar",
    body: "Kein langes Warten beim Start. Öffnen, anmelden, spielen — meist bist du schon in der Welt, bevor andere noch beim Ladebildschirm sind.",
  },
  {
    icon: I.feather,
    title: "Leicht für deinen PC",
    body: "Braucht spürbar weniger Arbeitsspeicher — das heißt weniger Hitze, weniger Lüfterlärm und ein flüssiges Spiel auch auf älteren Laptops.",
  },
  {
    icon: I.layers,
    title: "Alles an einem Ort",
    body: "Deine Konten, Lieblingsserver und die wichtigsten Einstellungen direkt im Launcher. Ein Klick verbindet dich mit deinem Server.",
  },
];

const STEPS = [
  { t: "Herunterladen", d: "Den kleinen Launcher laden und in wenigen Sekunden installieren. Ohne Zusatzsoftware." },
  { t: "Mit Microsoft anmelden", d: "Sichere Anmeldung mit deinem bestehenden Konto. Dein Passwort bleibt bei Microsoft." },
  { t: "Losspielen", d: "Auf „Spielen“ klicken — der Rest passiert automatisch, und du bist immer auf dem neuesten Stand." },
];

const FAQ = [
  {
    q: "Ist das fair — oder ist das schummeln?",
    a: "Weder noch: DolphinClient spielt ganz normales Minecraft 26.1 auf echten Servern — dieselben Regeln, Blöcke und Sounds. Es macht das Spiel schneller, nicht anders. Kein Singleplayer, keine Cheats.",
  },
  {
    q: "Kostet DolphinClient etwas?",
    a: "Nein. Der Launcher und das Spielen sind kostenlos. Du brauchst nur ein gekauftes Minecraft-Konto (Microsoft) — genau wie beim normalen Spiel.",
  },
  {
    q: "Läuft das auf meinem PC?",
    a: "Sehr wahrscheinlich. DolphinClient ist bewusst genügsam und läuft auch auf älteren Rechnern angenehm flüssig. Aktiv angeboten für Windows, dazu bei Bedarf macOS und Linux.",
  },
  {
    q: "Bleibe ich automatisch aktuell?",
    a: "Ja. Der Launcher hält sich und das Spiel selbst auf dem neuesten Stand — du musst nie manuell etwas nachladen oder aktualisieren.",
  },
  {
    q: "Woher kommen die Vergleichszahlen?",
    a: "Aus eigenen Messungen auf typischer Hardware. Wie groß der Unterschied bei dir ausfällt, hängt von deinem PC und der Szene ab — deshalb sind es Richtwerte, keine Versprechen.",
  },
];

export default function HomePage() {
  return (
    <main>
      {/* ---------- HERO ---------- */}
      <section className="hero">
        <div className="hero__grid">
          <div>
            <span className="kicker">Für Minecraft 26.1</span>
            <h1>
              Dein Minecraft.
              <br />
              <span className="accent">Spürbar schneller.</span>
            </h1>
            <p className="hero__lede">
              Mehr Bilder pro Sekunde, kürzere Ladezeiten und ein Spiel, das
              leicht auf deinem PC liegt. Ein Klick — und du spielst.
            </p>
            <p className="hero__note">
              Dieselben Server, dieselben Regeln, nur flüssiger. Kein Umbau, kein
              Basteln, keine Vorkenntnisse nötig.
            </p>

            <div className="cta">
              <Link className="btn lg" href="/download">Kostenlos laden</Link>
              <Link className="btn ghost lg" href="/features">Den Unterschied sehen</Link>
            </div>

            <div className="hero__meta">
              <div className="m"><b>3×</b><span>mehr FPS</span></div>
              <div className="m"><b>1 Klick</b><span>zum Spielen</span></div>
              <div className="m"><b>0 €</b><span>Kosten</span></div>
            </div>
          </div>

          {/* live performance readout (above the fold → always visible) */}
          <div className="readout">
            <div className="readout__top">
              <Logo />
              <span>leistung · live</span>
              <span className="readout__dot" />
            </div>
            <div className="readout__fps">
              <span className="big">318</span>
              <span className="unit">FPS · flüssig</span>
            </div>
            <div className="readout__eq">
              {Array.from({ length: 14 }).map((_, i) => (
                <span key={i} style={{ animationDelay: `${(i % 5) * 0.12}s` }} />
              ))}
            </div>
            <div className="readout__rows">
              <div className="readout__row"><span className="k">ladezeit</span><span className="l" /><span className="v good">4,8 s</span></div>
              <div className="readout__row"><span className="k">speicher</span><span className="l" /><span className="v good">1,4 GB</span></div>
              <div className="readout__row"><span className="k">bild</span><span className="l" /><span className="v">gestochen scharf</span></div>
              <div className="readout__row"><span className="k">status</span><span className="l" /><span className="v good">bereit zum Spielen</span></div>
            </div>
          </div>
        </div>
      </section>

      {/* ---------- COMPARISON (centerpiece) ---------- */}
      <Reveal as="section" className="sec-head">
        <span className="sec-head__idx">[ 01 · Der Unterschied ]</span>
        <span className="kicker">Seite an Seite</span>
        <h2 className="sec-title">Was du sofort merkst</h2>
        <p className="sec-lede">
          Gleicher PC, gleiche Welt, gleicher Moment — einmal mit DolphinClient,
          einmal ohne. Drei Zahlen, die den Alltag verändern.
        </p>
      </Reveal>
      <Compare />
      <p className="notice">
        Richtwerte aus eigenen Messungen auf typischer Hardware. Der tatsächliche
        Unterschied hängt von deinem PC und der Spielszene ab.
      </p>

      {/* ---------- BENEFITS ---------- */}
      <Reveal as="section" className="sec-head">
        <span className="sec-head__idx">[ 02 · Vorteile ]</span>
        <span className="kicker">Warum es sich lohnt</span>
        <h2 className="sec-title">Vier Dinge, die du liebst</h2>
      </Reveal>
      <section className="sheet">
        <div className="sheet__grid">
          {BENEFITS.map((f, i) => (
            <Reveal key={f.title} variant="up" delay={(i % 2) * 80}>
              <div className="cell">
                <span className="cell__idx">{String(i + 1).padStart(2, "0")}</span>
                <span className="cell__icon">{f.icon}</span>
                <h3>{f.title}</h3>
                <p>{f.body}</p>
              </div>
            </Reveal>
          ))}
        </div>
      </section>

      {/* ---------- STEPS ---------- */}
      <Reveal as="section" className="sec-head">
        <span className="sec-head__idx">[ 03 · So einfach ]</span>
        <span className="kicker">In unter einer Minute</span>
        <h2 className="sec-title">Von Download bis Spielstart</h2>
        <p className="sec-lede">Drei Schritte, kein Fachwissen. Wirklich.</p>
      </Reveal>
      <section className="steps">
        {STEPS.map((st, i) => (
          <Reveal key={st.t} variant="up" delay={i * 90}>
            <div className="step">
              <span className="step__n">Schritt {String(i + 1).padStart(2, "0")}</span>
              <h3>{st.t}</h3>
              <p>{st.d}</p>
            </div>
          </Reveal>
        ))}
      </section>

      {/* ---------- METRICS BAND ---------- */}
      <Reveal as="section" variant="up" className="band">
        <div className="band__grid">
          <div className="band__cell"><div className="band__num">bis 3×</div><div className="band__label">mehr FPS</div></div>
          <div className="band__cell"><div className="band__num">Sekunden</div><div className="band__label">bis spielbereit</div></div>
          <div className="band__cell"><div className="band__num">−46 %</div><div className="band__label">Arbeitsspeicher</div></div>
          <div className="band__cell"><div className="band__num">0 €</div><div className="band__label">kostenlos</div></div>
        </div>
      </Reveal>

      {/* ---------- FAQ ---------- */}
      <Reveal as="section" className="sec-head">
        <span className="sec-head__idx">[ 04 · Fragen ]</span>
        <span className="kicker">Kurz & ehrlich</span>
        <h2 className="sec-title">Häufige Fragen</h2>
      </Reveal>
      <section className="faq">
        {FAQ.map((f, i) => (
          <Reveal key={f.q} variant="fade" delay={i * 40}>
            <details open={i === 0}>
              <summary><span className="q-idx">{String(i + 1).padStart(2, "0")}</span>{f.q}</summary>
              <p>{f.a}</p>
            </details>
          </Reveal>
        ))}
      </section>

      {/* ---------- CTA ---------- */}
      <Reveal as="section" variant="zoom" className="cta-band">
        <span className="kicker">Bereit?</span>
        <h2>Spür den Unterschied selbst.</h2>
        <p>Laden, anmelden, spielen — in unter einer Minute. Kostenlos, ohne Risiko, jederzeit wieder deinstallierbar.</p>
        <div className="cta">
          <Link className="btn lg" href="/download">Jetzt kostenlos laden</Link>
          <Link className="btn ghost lg" href="/features">Vorteile ansehen</Link>
        </div>
      </Reveal>
    </main>
  );
}
