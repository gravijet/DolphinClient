import Link from "next/link";
import Reveal from "./components/Reveal";
import Counter from "./components/Counter";
import TiltCard from "./components/TiltCard";
import Logo from "./components/Logo";

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
  sound: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 9v6h4l5 4V5L8 9H4Z" />
      <path d="M16.5 8.5a5 5 0 0 1 0 7M19 6a8.5 8.5 0 0 1 0 12" />
    </svg>
  ),
  globe: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="9" />
      <path d="M3 12h18M12 3c2.5 2.6 3.8 5.7 3.8 9S14.5 18.4 12 21c-2.5-2.6-3.8-5.7-3.8-9S9.5 5.6 12 3Z" />
    </svg>
  ),
  server: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="4" width="18" height="7" rx="2" />
      <rect x="3" y="13" width="18" height="7" rx="2" />
      <path d="M7 7.5h.01M7 16.5h.01" />
    </svg>
  ),
  sliders: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0" />
      <circle cx="16" cy="6" r="2" /><circle cx="10" cy="12" r="2" /><circle cx="18" cy="18" r="2" />
    </svg>
  ),
  user: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="8" r="4" />
      <path d="M4 21c0-4 4-6 8-6s8 2 8 6" />
    </svg>
  ),
  chart: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 20V4M4 20h16" />
      <rect x="7" y="12" width="3" height="5" rx="0.6" fill="currentColor" stroke="none" />
      <rect x="12" y="8" width="3" height="9" rx="0.6" fill="currentColor" stroke="none" />
      <rect x="17" y="10" width="3" height="7" rx="0.6" fill="currentColor" stroke="none" />
    </svg>
  ),
};

const FEATURES = [
  { icon: I.bolt, cls: "", title: "Native Engine", body: "Kein Java, kein Fabric: DolphinClient rendert Minecraft 26.1 selbst in Rust (wgpu). Hohe FPS sind eingebaut, nicht nachgerüstet." },
  { icon: I.toggle, cls: "violet", title: "Uncapped FPS", body: "VSync abschaltbar für maximale Frames, oder ein FPS-Limit setzen. Render-Distanz, GUI-Skalierung, Helligkeit und mehr — alles live einstellbar." },
  { icon: I.sound, cls: "pink", title: "Echter Vanilla-Sound", body: "Originale Mojang-Sounds — Blöcke, Schritte, Mobs, Musik. On-demand von Mojang geladen, mit vollem Lautstärke-Menü pro Kategorie." },
  { icon: I.globe, cls: "", title: "1:1 Multiplayer", body: "Kein Singleplayer, keine Spielereien: DolphinClient verbindet dich mit echten 26.1-Servern — dieselben Blöcke, Sounds und Regeln wie im Original." },
  { icon: I.rocket, cls: "gold", title: "Multi-Account-Launcher", body: "Mehrere Microsoft-Konten verwalten, blitzschnell wechseln — und bestehende Logins aus Vanilla- & Lunar-Launcher automatisch importieren." },
  { icon: I.refresh, cls: "violet", title: "Immer die neueste Version", body: "Der Launcher prüft bei jedem Start die Signatur des Clients und lädt automatisch die aktuellste Version nach — du spielst nie eine veraltete Build." },
  { icon: I.cape, cls: "pink", title: "Cosmetics", body: "Capes im Launcher und im Dashboard wählbar. Das In-Game-Rendering ist der nächste Schritt — ehrlich statt versprochen." },
  { icon: I.chip, cls: "", title: "Volles Vanilla-Menü", body: "Titelbildschirm, Optionen mit Video-/Steuerungs-/Chat-/Sound-Untermenüs und ein Esc-Pausenmenü — genau wie im echten Minecraft." },
];

const NEW13 = [
  { icon: I.server, cls: "", title: "Server-Liste im Launcher", body: "Speichere deine Lieblingsserver, lege einen Standard fest und tritt mit einem Klick direkt bei — kein Umweg über den Verbindungsbildschirm." },
  { icon: I.sliders, cls: "violet", title: "Spiel-Schnelleinstellungen", body: "Render-Distanz, FPS-Limit, Sichtfeld, Helligkeit, VSync, GUI-Skalierung, Grafik-Preset & Discord — direkt im Launcher, wirkt beim nächsten Start." },
  { icon: I.user, cls: "pink", title: "Skin-Vorschau & Aktivität", body: "Dein Ganzkörper-Skin direkt im Profil, dazu eine Spielzeit-Historie deiner letzten Sitzungen als Sparkline." },
  { icon: I.chart, cls: "gold", title: "Erweitertes Dashboard", body: "Server, Aktivitäts-Diagramm und Spiel-Einstellungen live aus dem laufenden Launcher — ohne Login, direkt im Browser." },
];

// Honest side-by-side: a native client vs. the vanilla Java launcher.
const COMPARE: { label: string; dolphin: string | boolean; vanilla: string | boolean }[] = [
  { label: "Engine", dolphin: "Rust + wgpu (nativ)", vanilla: "Java / JVM" },
  { label: "Java nötig", dolphin: false, vanilla: "Ja (JDK 25)" },
  { label: "Launcher-Kaltstart", dolphin: "< 0,5 s", vanilla: "mehrere Sekunden" },
  { label: "Mehrere Accounts", dolphin: true, vanilla: true },
  { label: "Server-Liste im Launcher", dolphin: true, vanilla: false },
  { label: "Spiel-Einstellungen im Launcher", dolphin: true, vanilla: false },
  { label: "Auto-Update des Clients", dolphin: "Ja (SHA-256)", vanilla: true },
  { label: "Live-Web-Dashboard", dolphin: true, vanilla: false },
];

const STEPS = [
  { title: "Launcher laden", body: "Lade das native DolphinClient-Setup für dein System herunter und installiere es in Sekunden." },
  { title: "Mit Microsoft anmelden", body: "Sichere Anmeldung über den offiziellen Microsoft-Login. Kein Passwort verlässt je deinen Rechner." },
  { title: "Ein Klick", body: "Der Launcher lädt die Original-Texturen von Mojang und den nativen Client und hält alles aktuell." },
  { title: "Spielen", body: "Der native Client startet mit hohen FPS und deinen Cosmetics — sofort spielbereit." },
];

const MODS = ["Native Rendering", "Hohe FPS", "Schneller Start", "Wenig RAM", "Eigenes Protokoll", "Server-Liste", "Quick-Settings", "Skin-Vorschau", "Cosmetics", "HUD"];

const FAQ = [
  { q: "Ist DolphinClient kostenlos?", a: "Ja. Der Client, der Launcher und die Basis-Cosmetics sind kostenlos. Du brauchst nur ein gekauftes Minecraft-Konto." },
  { q: "Gibt es Singleplayer?", a: "Nein — bewusst nicht. DolphinClient ist ein 1:1-Multiplayer-Client: er verbindet dich mit echten Minecraft-26.1-Servern, mit denselben Blöcken, Sounds und Regeln wie im Original. Kein abweichender Singleplayer-Modus, keine Extras, die dich vom echten Spiel entfernen." },
  { q: "Hat der Client echten Sound?", a: "Ja. DolphinClient spielt die originalen Mojang-Sounds ab — Blöcke, Schritte, Mobs, Musik und mehr. Die Sounddateien werden bei Bedarf direkt von Mojang nachgeladen, und im Menü stellst du die Lautstärke pro Kategorie ein (Master, Musik, Blöcke, Kreaturen …)." },
  { q: "Spiele ich immer die neueste Version?", a: "Ja. Der Launcher vergleicht bei jedem Start die Signatur (SHA-256) deines Clients mit der aktuellen Veröffentlichung und lädt bei Bedarf automatisch die neueste Version nach. Eine veraltete Build kann so nicht hängenbleiben." },
  { q: "Kann ich mehrere Accounts nutzen?", a: "Ja. Der Launcher verwaltet beliebig viele Microsoft-Konten — hinzufügen, wechseln und entfernen mit einem Klick. Bestehende Logins aus dem Vanilla- und Lunar-Launcher werden automatisch erkannt und importiert." },
  { q: "Brauche ich ein Minecraft-Konto?", a: "Ja — ein gültiges Microsoft/Minecraft-Konto. Der Launcher lädt die Original-Texturen direkt von Mojang. Keine Cracked-Accounts." },
  { q: "Brauche ich Java?", a: "Nein. DolphinClient ist eine komplett native Engine in Rust — kein Java, kein Fabric. Du brauchst nur eine GPU mit Vulkan, Metal oder DirectX 12." },
  { q: "Warum ist der Client nativ?", a: "Ein nativer Rust-Client startet quasi sofort, braucht wenig RAM und liefert sehr hohe FPS — auch auf schwachen PCs. Kein JVM-Warmup, kein Modpack-Overhead." },
];

function Cell({ value }: { value: string | boolean }) {
  if (value === true) return <span className="compare__yes">{I.check}</span>;
  if (value === false) return <span className="compare__no" aria-label="nein">✕</span>;
  return <span>{value}</span>;
}

export default function HomePage() {
  return (
    <main>
      {/* ---------- HERO ---------- */}
      <section className="hero">
        <div className="hero__grid">
          <div>
            <span className="hero__eyebrow">
              <span className="dot" /> Nativer Client · Minecraft 26.1
            </span>
            <h1>
              Minecraft 26.1, neu gebaut in Rust.
              <br />
              <span className="glow">DolphinClient.</span>
            </h1>
            <p className="tagline">
              Kein Java, kein Fabric, kein Modpack. DolphinClient rendert die Welt
              selbst — eigener wgpu-Renderer, eigenes Protokoll, echte
              Mojang-Texturen und -Sounds. Ein Launcher meldet dich an und hält
              alles aktuell.
            </p>
            <p className="honest">
              Was das konkret heißt: Der Client ist ein eigenständiges Programm,
              nicht eine Mod in Minecraft. Er verbindet sich 1:1 mit echten
              26.1-Servern — dieselben Blöcke, Sounds und Regeln. Kein
              Singleplayer, keine Cheats. Und das Web-Dashboard hier verbindet
              sich live mit deinem laufenden Launcher, statt Zahlen zu erfinden.
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
                <span>Login &amp; Updates</span>
              </div>
            </div>
          </div>

          {/* launcher preview mockup */}
          <Reveal variant="left" delay={120}>
            <div className="hero__preview">
              <div className="hero__preview-bar">
                <i /><i /><i />
                <Logo />
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
                  <div className="hud__row"><span className="k">Version</span><span className="v good">26.1 · aktuell</span></div>
                  <div className="hud__row"><span className="k">Sound</span><span className="v good">Vanilla · an</span></div>
                  <div className="hud__row"><span className="k">Dashboard</span><span className="v good">● live verbunden</span></div>
                  <div className="hud__row"><span className="k">Konten</span><span className="v">3 · Steve aktiv</span></div>
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
          Kein FPS-Wunder aus dem Nichts — sondern eine native Engine in Rust,
          plus die Extras eines Premium-Clients.
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

      {/* ---------- NEW IN 0.13.0 ---------- */}
      <Reveal as="section" className="section-head">
        <span className="eyebrow-sm">Neu in v0.13.0</span>
        <h2 className="section-title">Der Launcher wird zur Kommandozentrale</h2>
        <p className="section-sub">
          Server, Spiel-Einstellungen und dein Profil an einem Ort — und das
          Web-Dashboard zeigt alles live mit.
        </p>
      </Reveal>
      <section className="card-grid">
        {NEW13.map((f, i) => (
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
        <p className="section-sub">Kein Fummeln mit Java, Fabric oder Configs. Der Launcher lädt den nativen Client und startet ihn.</p>
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
            <div className="band__num"><Counter to={6} suffix=" MB" /></div>
            <div className="band__label">Launcher-Größe</div>
          </div>
          <div>
            <div className="band__num"><Counter to={14} /></div>
            <div className="band__label">Einstellbare Optionen</div>
          </div>
        </div>
      </Reveal>

      {/* ---------- COMPARISON ---------- */}
      <Reveal as="section" className="section-head">
        <span className="eyebrow-sm">Im Vergleich</span>
        <h2 className="section-title">Nativ statt JVM</h2>
        <p className="section-sub">
          Ehrlich gegenübergestellt — was ein nativer Client anders macht als der
          klassische Java-Launcher.
        </p>
      </Reveal>
      <Reveal as="section" variant="up" className="compare-wrap">
        <table className="compare">
          <thead>
            <tr>
              <th />
              <th className="is-us">DolphinClient</th>
              <th>Vanilla-Launcher</th>
            </tr>
          </thead>
          <tbody>
            {COMPARE.map((row) => (
              <tr key={row.label}>
                <td className="compare__label">{row.label}</td>
                <td className="is-us"><Cell value={row.dolphin} /></td>
                <td><Cell value={row.vanilla} /></td>
              </tr>
            ))}
          </tbody>
        </table>
      </Reveal>

      {/* ---------- SPLIT: PERFORMANCE ---------- */}
      <Reveal as="section" variant="up" className="split" style={{}}>
        <div>
          <span className="eyebrow-sm">Performance & HUD</span>
          <h3>FPS, die man sieht — und ein HUD, das hilft</h3>
          <p>
            Der native Renderer holt aus jedem Frame das Maximum — parallel
            gemeshte Chunks, kein JVM-Overhead. Das F3-Debug-HUD zeigt dir genau,
            was zählt, ohne Leistung zu kosten.
          </p>
          <ul>
            <li>{I.check}<span>F3-Overlay: FPS, Koordinaten &amp; Blickrichtung, Leben, geladene Chunks</span></li>
            <li>{I.check}<span>VSync aus für uncapped FPS — oder eigenes FPS-Limit setzen</span></li>
            <li>{I.check}<span>Esc-Pausenmenü &amp; volles Optionsmenü — genau wie Vanilla</span></li>
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
            <div className="hud__row"><span className="k">Binärgröße</span><span className="v">~6 MB</span></div>
            <div className="hud__row"><span className="k">RAM (idle)</span><span className="v">~30 MB</span></div>
            <div className="hud__row"><span className="k">Start</span><span className="v good">&lt; 0.5 s</span></div>
          </div>
        </div>
        <div>
          <span className="eyebrow-sm">Nativer Launcher</span>
          <h3>Ein echtes Windows-Programm — mit mehreren Accounts</h3>
          <p>
            Der neue Launcher ist in Rust geschrieben und rendert nativ: ein
            einziges, winziges .exe, das quasi sofort startet. Verwalte mehrere
            Microsoft-Konten, wechsle mit einem Klick — oder importiere bestehende
            Logins aus anderen Launchern auf deinem Gerät.
          </p>
          <ul>
            <li>{I.check}<span>Mehrere Accounts hinzufügen, wechseln &amp; entfernen</span></li>
            <li>{I.check}<span>Auto-Import aus Vanilla- &amp; Lunar-Launcher (bereits angemeldete Konten)</span></li>
            <li>{I.check}<span>Sichere Token-Ablage über die Windows-Keychain (DPAPI)</span></li>
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
        <small>* Grober Richtwert je nach Hardware und Szene. Die Performance kommt aus dem nativen Rust-Renderer (wgpu).</small>
      </p>
    </main>
  );
}
