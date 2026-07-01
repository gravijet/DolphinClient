import Link from "next/link";

const FEATURES = [
  {
    title: "Performance out of the box",
    body: "Sodium, Lithium & Co. sind vorkonfiguriert — flüssige FPS ohne Gefummel an Config-Dateien.",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M13 2 3 14h9l-1 8 10-12h-9l1-8Z" />
      </svg>
    ),
  },
  {
    title: "Module an/aus",
    body: "Aktiviere nur, was du brauchst. Deaktivierte Module kosten keine Leistung.",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <rect x="2" y="7" width="20" height="10" rx="5" />
        <circle cx="9" cy="12" r="3" fill="currentColor" stroke="none" />
      </svg>
    ),
  },
  {
    title: "Cosmetics",
    body: "Capes und mehr — sichtbar für andere DolphinClient-Nutzer im Spiel.",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M6 3v10c0 3 2.7 5 6 5s6-2 6-5V3" />
        <path d="M6 3c0 2 2.7 3 6 3s6-1 6-3" />
      </svg>
    ),
  },
  {
    title: "Ein Klick, ein Launcher",
    body: "Microsoft-Login, Fabric-Setup und Auto-Update — der Launcher erledigt alles automatisch.",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M12 3v12" />
        <path d="m7 10 5 5 5-5" />
        <path d="M5 21h14" />
      </svg>
    ),
  },
];

export default function HomePage() {
  return (
    <main>
      <section className="hero">
        <span className="hero__eyebrow">Performance · Minecraft 26.1</span>
        <h1>
          Mehr FPS, weniger Aufwand.
          <br />
          <span className="glow">DolphinClient.</span>
        </h1>
        <p className="tagline">
          Vorkonfigurierte Performance-Mods, Cosmetics und ein Launcher mit
          Microsoft-Login — für ein flüssiges Minecraft 26.1.
        </p>
        <p className="honest">
          Ehrlich gesagt: Die FPS kommen aus erstklassigen Open-Source-Mods. Wir
          bündeln sie bequem an einem Ort und bringen Cosmetics &amp; Community
          dazu — ein Klick, und es läuft.
        </p>

        <div className="cta">
          <Link className="btn" href="/download">
            Herunterladen
          </Link>
          <Link className="btn ghost" href="/account">
            Account &amp; Cosmetics
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
      </section>

      <h2 className="section-title">Warum DolphinClient?</h2>
      <p className="section-sub">
        Kein FPS-Wunder aus dem Nichts — sondern die besten Mods, bequem
        gebündelt, plus die Extras eines großen Clients.
      </p>

      <section className="features">
        {FEATURES.map((f) => (
          <div key={f.title}>
            <span className="puck">{f.icon}</span>
            <h3>{f.title}</h3>
            <p>{f.body}</p>
          </div>
        ))}
      </section>
    </main>
  );
}
