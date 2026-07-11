import Link from "next/link";
import Reveal from "./components/Reveal";
import Counter from "./components/Counter";
import Logo from "./components/Logo";

/* -------- line icons (thin, technical — no filled "pucks") -------- */
const s = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.6,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};
const I = {
  cpu: (
    <svg viewBox="0 0 24 24" {...s}>
      <rect x="7" y="7" width="10" height="10" rx="1.5" />
      <path d="M9.5 9.5h5v5h-5z" opacity="0.5" />
      <path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3" />
    </svg>
  ),
  net: (
    <svg viewBox="0 0 24 24" {...s}>
      <circle cx="5" cy="12" r="2.2" /><circle cx="19" cy="6" r="2.2" /><circle cx="19" cy="18" r="2.2" />
      <path d="M6.9 11 17 6.6M6.9 13 17 17.4" />
    </svg>
  ),
  assets: (
    <svg viewBox="0 0 24 24" {...s}>
      <path d="M12 3 3.5 7.5v9L12 21l8.5-4.5v-9L12 3Z" />
      <path d="M3.6 7.6 12 12l8.4-4.4M12 12v9" opacity="0.7" />
    </svg>
  ),
  noJava: (
    <svg viewBox="0 0 24 24" {...s}>
      <circle cx="12" cy="12" r="9" />
      <path d="M6 6l12 12" />
    </svg>
  ),
  sliders: (
    <svg viewBox="0 0 24 24" {...s}>
      <path d="M4 7h9M18 7h2M4 12h4M12 12h8M4 17h13M20 17h0" />
      <circle cx="15" cy="7" r="1.9" /><circle cx="9" cy="12" r="1.9" /><circle cx="18" cy="17" r="1.9" />
    </svg>
  ),
  sound: (
    <svg viewBox="0 0 24 24" {...s}>
      <path d="M4 9v6h4l5 4V5L8 9H4Z" />
      <path d="M16.5 8.5a5 5 0 0 1 0 7" />
    </svg>
  ),
  globe: (
    <svg viewBox="0 0 24 24" {...s}>
      <circle cx="12" cy="12" r="9" />
      <path d="M3 12h18M12 3c2.5 2.6 3.8 5.7 3.8 9S14.5 18.4 12 21c-2.5-2.6-3.8-5.7-3.8-9S9.5 5.6 12 3Z" />
    </svg>
  ),
  shield: (
    <svg viewBox="0 0 24 24" {...s}>
      <path d="M12 3 5 6v5c0 4 3 7.5 7 9 4-1.5 7-5 7-9V6l-7-3Z" />
      <path d="m9 12 2 2 4-4" />
    </svg>
  ),
  check: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
      <path d="m20 6-11 11-5-5" />
    </svg>
  ),
};

const FEATURES = [
  { icon: I.cpu, title: "Eigener Renderer", body: "Ein in Rust geschriebener wgpu-Renderer spricht Vulkan, DirectX 12 oder Metal direkt an — kein OpenGL-über-Java-Umweg." },
  { icon: I.net, title: "Eigenes Protokoll", body: "Der Client redet das 26.1-Netzwerkprotokoll selbst — keine Übersetzungsschicht, keine Bastion aus Mod-Loadern dazwischen." },
  { icon: I.assets, title: "Echte Vanilla-Assets", body: "Texturen, Blockmodelle und Sounds stammen im Original von Mojang und werden bei Bedarf geladen. Du besitzt das Spiel, du siehst das Spiel." },
  { icon: I.noJava, title: "Kein Java", body: "Keine JVM, kein Fabric, kein Modpack, kein Warmup. Ein einziges natives Programm, das startet und läuft." },
  { icon: I.sliders, title: "Voll einstellbar", body: "Render-Distanz, FPS-Limit, Sichtfeld, GUI-Skalierung, Helligkeit, VSync — im Spielmenü und vorab im Launcher." },
  { icon: I.sound, title: "Echter Sound", body: "Die originalen Mojang-Sounds — Blöcke, Schritte, Kreaturen, Musik. Lautstärke pro Kategorie, wie im echten Menü." },
  { icon: I.globe, title: "1:1 Multiplayer", body: "Verbindet mit echten Minecraft-26.1-Servern: dieselben Regeln, Blöcke und Sounds. Kein Singleplayer, keine Cheats." },
  { icon: I.shield, title: "Hält sich aktuell", body: "Der Launcher prüft bei jedem Start die SHA-256-Signatur des Clients und zieht bei Bedarf die neueste Build nach." },
];

const STEPS = [
  { t: "Launcher beziehen", d: "Das native Setup laden und in Sekunden installieren — keine Laufzeit, keine Abhängigkeiten." },
  { t: "Mit Microsoft anmelden", d: "Anmeldung über den offiziellen Microsoft-Flow. Kein Passwort verlässt je deinen Rechner." },
  { t: "Assets holen", d: "Der Launcher lädt die Original-Assets von Mojang und den nativen Client — einmalig, dann gecacht." },
  { t: "Spielen", d: "Der Client startet nativ, verbindet direkt mit deinem Server und hält alles auf dem neuesten Stand." },
];

const COMPARE: { label: string; us: string | boolean; them: string | boolean }[] = [
  { label: "Ausführung", us: "Rust · nativ", them: "Java / JVM" },
  { label: "Renderer", us: "wgpu (Vulkan/DX12/Metal)", them: "OpenGL über LWJGL" },
  { label: "Java erforderlich", us: false, them: "JDK 25" },
  { label: "Launcher-Kaltstart", us: "0,42 s", them: "mehrere Sekunden" },
  { label: "Server im Launcher", us: true, them: false },
  { label: "Spiel-Optionen im Launcher", us: true, them: false },
  { label: "Client-Auto-Update", us: "SHA-256", them: true },
  { label: "Live-Web-Dashboard", us: true, them: false },
];

const FAQ = [
  { q: "Ist das eine Mod oder ein Modpack?", a: "Weder noch. DolphinClient ist ein eigenständiges Programm — eine komplette Minecraft-26.1-Engine in Rust. Es lädt nicht Minecraft und hängt sich nicht ein, es ist der Client." },
  { q: "Kostet DolphinClient etwas?", a: "Nein. Client, Launcher und die Basis-Cosmetics sind kostenlos. Du brauchst nur ein gekauftes Minecraft-Konto (Microsoft), weil die Original-Assets direkt von Mojang geladen werden." },
  { q: "Gibt es Singleplayer oder Cheats?", a: "Bewusst nicht. DolphinClient ist ein 1:1-Multiplayer-Client für echte 26.1-Server — dieselben Blöcke, Sounds und Regeln wie im Original. Kein abweichender Modus, der dich vom echten Spiel entfernt." },
  { q: "Brauche ich Java?", a: "Nein. Es gibt keine JVM und kein Fabric. Du brauchst nur eine halbwegs aktuelle GPU mit Vulkan, Metal oder DirectX 12." },
  { q: "Spiele ich immer die neueste Version?", a: "Ja. Der Launcher vergleicht bei jedem Start die SHA-256-Signatur deines Clients mit der Veröffentlichung und zieht bei Bedarf automatisch nach. Eine veraltete Build kann nicht hängenbleiben." },
  { q: "Kann ich mehrere Konten nutzen?", a: "Ja. Der Launcher verwaltet beliebig viele Microsoft-Konten — hinzufügen, wechseln, entfernen. Bereits angemeldete Konten aus dem Vanilla- und Lunar-Launcher werden automatisch erkannt und importiert." },
  { q: "Was hat es mit dem Dashboard auf sich?", a: "Das Dashboard auf dieser Seite verbindet sich lokal mit deinem laufenden Launcher (127.0.0.1) und liest live aktives Konto, Version, Spielzeit und Einstellungen. Kein zweiter Login, kein Server dazwischen, keine erfundenen Zahlen." },
];

function Cell({ value }: { value: string | boolean }) {
  if (value === true) return <span className="compare__yes">{I.check}</span>;
  if (value === false) return <span className="compare__no" aria-label="nein">—</span>;
  return <span>{value}</span>;
}

export default function HomePage() {
  return (
    <main>
      {/* ---------- HERO ---------- */}
      <section className="hero">
        <div className="hero__grid">
          <div>
            <span className="kicker">Native Engine · Minecraft 26.1</span>
            <h1>
              Minecraft, von Grund
              <br />
              auf neu gebaut.
              <br />
              <span className="accent">In Rust.</span>
            </h1>
            <p className="hero__lede">
              DolphinClient ist keine Mod und kein Modpack, sondern eine
              eigenständige Spiel-Engine: eigener Renderer, eigenes Protokoll,
              echte Mojang-Assets. Dazu ein winziger nativer Launcher und ein
              Dashboard, das live mitläuft.
            </p>
            <p className="hero__note">
              Zur Einordnung: ein eigenständiges Programm, nicht eine Mod in
              Minecraft. Es verbindet sich 1:1 mit echten 26.1-Servern — dieselben
              Blöcke, Sounds und Regeln. Kein Singleplayer, keine Cheats.
            </p>

            <div className="cta">
              <Link className="btn lg" href="/download">Beziehen</Link>
              <Link className="btn ghost lg" href="/features">Technik ansehen</Link>
            </div>

            <div className="hero__meta">
              <div className="m"><b>0,42 s</b><span>Kaltstart</span></div>
              <div className="m"><b>~6 MB</b><span>Launcher</span></div>
              <div className="m"><b>0</b><span>Zeilen Java</span></div>
            </div>
          </div>

          {/* instrument readout */}
          <Reveal variant="left" delay={120}>
            <div className="spec">
              <div className="spec__head">
                <span className="dot" />
                <Logo />
                client.status
                <span className="spec__tag">26.1</span>
              </div>
              <div className="spec__body">
                <div className="spec__row"><span className="spec__k">renderer</span><span className="spec__leader" /><span className="spec__v">wgpu · vulkan</span></div>
                <div className="spec__row"><span className="spec__k">protokoll</span><span className="spec__leader" /><span className="spec__v">eigen · 26.1</span></div>
                <div className="spec__row"><span className="spec__k">sprache</span><span className="spec__leader" /><span className="spec__v aqua">rust</span></div>
                <div className="spec__row"><span className="spec__k">java</span><span className="spec__leader" /><span className="spec__v">nicht erforderlich</span></div>
                <div className="spec__row"><span className="spec__k">sound</span><span className="spec__leader" /><span className="spec__v">vanilla · mojang</span></div>
                <div className="spec__row"><span className="spec__k">status</span><span className="spec__leader" /><span className="spec__v good">● spielbereit</span></div>
              </div>
              <div className="spec__foot">▶ Spielen (26.1)</div>
            </div>
          </Reveal>
        </div>
      </section>

      {/* ---------- FEATURES ---------- */}
      <Reveal as="section" className="sec-head">
        <span className="sec-head__idx">[ 01 / Übersicht ]</span>
        <span className="kicker">Was drinsteckt</span>
        <h2 className="sec-title">Eine Engine, kein Aufsatz</h2>
        <p className="sec-lede">
          Kein FPS-Wunder aus dem Nichts, sondern die naheliegende Konsequenz aus
          nativem Code: acht Dinge, die DolphinClient anders macht als ein
          klassischer Java-Client.
        </p>
      </Reveal>
      <section className="sheet">
        <div className="sheet__grid">
          {FEATURES.map((f, i) => (
            <Reveal key={f.title} variant="up" delay={(i % 4) * 60}>
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

      {/* ---------- HOW ---------- */}
      <Reveal as="section" className="sec-head">
        <span className="sec-head__idx">[ 02 / Ablauf ]</span>
        <span className="kicker">In vier Schritten</span>
        <h2 className="sec-title">Von Download bis Spielstart</h2>
        <p className="sec-lede">Kein Fummeln mit Java, Fabric oder Configs. Beziehen, anmelden, spielen.</p>
      </Reveal>
      <section className="steps">
        {STEPS.map((st, i) => (
          <Reveal key={st.t} variant="up" delay={i * 70}>
            <div className="step">
              <span className="step__n">{String(i + 1).padStart(2, "0")} —</span>
              <h3>{st.t}</h3>
              <p>{st.d}</p>
            </div>
          </Reveal>
        ))}
      </section>

      {/* ---------- METRICS ---------- */}
      <Reveal as="section" variant="up" className="band">
        <div className="band__grid">
          <div className="band__cell">
            <div className="band__num"><Counter to={0.42} decimals={2} group suffix=" s" /></div>
            <div className="band__label">Launcher-Kaltstart</div>
          </div>
          <div className="band__cell">
            <div className="band__num"><Counter to={6} suffix=" MB" /></div>
            <div className="band__label">Launcher-Binary</div>
          </div>
          <div className="band__cell">
            <div className="band__num">26.1</div>
            <div className="band__label">Minecraft-Ziel</div>
          </div>
          <div className="band__cell">
            <div className="band__num">0</div>
            <div className="band__label">Zeilen Java</div>
          </div>
        </div>
      </Reveal>

      {/* ---------- COMPARE ---------- */}
      <Reveal as="section" className="sec-head">
        <span className="sec-head__idx">[ 03 / Vergleich ]</span>
        <span className="kicker">Nativ gegen JVM</span>
        <h2 className="sec-title">Ehrlich gegenübergestellt</h2>
        <p className="sec-lede">Was ein nativer Client anders macht als der klassische Java-Launcher — Zeile für Zeile.</p>
      </Reveal>
      <Reveal as="section" variant="up" className="compare-wrap">
        <table className="compare">
          <thead>
            <tr>
              <th />
              <th className="is-us">DolphinClient</th>
              <th>Java-Vanilla</th>
            </tr>
          </thead>
          <tbody>
            {COMPARE.map((row) => (
              <tr key={row.label}>
                <td className="compare__label">{row.label}</td>
                <td className="is-us"><Cell value={row.us} /></td>
                <td><Cell value={row.them} /></td>
              </tr>
            ))}
          </tbody>
        </table>
      </Reveal>

      {/* ---------- SPLIT: engine/HUD ---------- */}
      <Reveal as="section" variant="up" className="split">
        <div>
          <span className="kicker">Renderer &amp; HUD</span>
          <h3>Frames, die man sieht — und ein HUD, das nicht bremst</h3>
          <p>
            Chunks werden parallel gemesht (rayon), es gibt kein JVM-Warmup und
            keinen Modpack-Overhead. Das F3-Debug-Overlay zeigt genau die Werte,
            die zählen, ohne Leistung zu kosten.
          </p>
          <ul>
            <li><span className="mk">F3</span><span>FPS, Koordinaten &amp; Blick, Leben, geladene Chunks</span></li>
            <li><span className="mk">VSYNC</span><span>abschaltbar für uncapped FPS — oder eigenes Limit</span></li>
            <li><span className="mk">MENU</span><span>volles Vanilla-Optionsmenü und Esc-Pause</span></li>
          </ul>
        </div>
        <div className="split__media">
          <div className="spec">
            <div className="spec__head"><span className="dot" /> f3.overlay<span className="spec__tag">live</span></div>
            <div className="spec__body">
              <div className="spec__row"><span className="spec__k">fps</span><span className="spec__leader" /><span className="spec__v good">312</span></div>
              <div className="spec__row"><span className="spec__k">xyz</span><span className="spec__leader" /><span className="spec__v">128 / 71 / -42</span></div>
              <div className="spec__row"><span className="spec__k">blick</span><span className="spec__leader" /><span className="spec__v">nord-ost</span></div>
              <div className="spec__row"><span className="spec__k">chunks</span><span className="spec__leader" /><span className="spec__v">441 / 441</span></div>
              <div className="spec__row"><span className="spec__k">session</span><span className="spec__leader" /><span className="spec__v">01:24:07</span></div>
            </div>
          </div>
        </div>
      </Reveal>

      {/* ---------- SPLIT: launcher ---------- */}
      <Reveal as="section" variant="up" className="split reverse">
        <div className="split__media">
          <div className="spec">
            <div className="spec__head"><span className="dot" /> launcher.build<span className="spec__tag">rust</span></div>
            <div className="spec__body">
              <div className="spec__row"><span className="spec__k">sprache</span><span className="spec__leader" /><span className="spec__v aqua">rust · egui</span></div>
              <div className="spec__row"><span className="spec__k">binary</span><span className="spec__leader" /><span className="spec__v">~6 MB</span></div>
              <div className="spec__row"><span className="spec__k">ram idle</span><span className="spec__leader" /><span className="spec__v">~30 MB</span></div>
              <div className="spec__row"><span className="spec__k">kaltstart</span><span className="spec__leader" /><span className="spec__v good">0,42 s</span></div>
              <div className="spec__row"><span className="spec__k">tokens</span><span className="spec__leader" /><span className="spec__v">os-keychain</span></div>
            </div>
          </div>
        </div>
        <div id="launcher">
          <span className="kicker">Der Launcher</span>
          <h3>Ein echtes Programm — kein verkleideter Browser</h3>
          <p>
            Der Launcher ist in Rust geschrieben und rendert nativ: ein einziges,
            winziges Binary, das quasi sofort startet. Kein mitgeliefertes
            Chromium, kein Node. Verwalte mehrere Microsoft-Konten und wechsle mit
            einem Klick.
          </p>
          <ul>
            <li><span className="mk">MULTI</span><span>Konten hinzufügen, wechseln &amp; entfernen</span></li>
            <li><span className="mk">IMPORT</span><span>bestehende Logins aus Vanilla- &amp; Lunar-Launcher</span></li>
            <li><span className="mk">SAFE</span><span>Refresh-Tokens in der OS-Keychain (DPAPI)</span></li>
          </ul>
          <div className="cta">
            <Link className="btn" href="/download">Launcher beziehen</Link>
            <Link className="btn ghost" href="/features#launcher">Mehr dazu</Link>
          </div>
        </div>
      </Reveal>

      {/* ---------- FAQ ---------- */}
      <Reveal as="section" className="sec-head">
        <span className="sec-head__idx">[ 04 / Fragen ]</span>
        <span className="kicker">Kurz &amp; ehrlich</span>
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
        <span className="kicker">Bereit fürs Wasser</span>
        <h2>Tauch ein in flüssiges 26.1.</h2>
        <p>Launcher beziehen, mit Microsoft anmelden, spielen — in unter einer Minute, ohne einen einzigen Java-Dialog.</p>
        <div className="cta">
          <Link className="btn lg" href="/download">Jetzt beziehen</Link>
          <Link className="btn ghost lg" href="/dashboard">Dashboard öffnen</Link>
        </div>
      </Reveal>

      <p className="notice" style={{ marginTop: "2.5rem" }}>
        Performance hängt von Hardware und Szene ab — die genannten Werte sind
        Richtwerte aus dem nativen Rust-Renderer (wgpu), keine Versprechen.
      </p>
    </main>
  );
}
