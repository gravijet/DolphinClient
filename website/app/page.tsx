import Link from "next/link";

export default function HomePage() {
  return (
    <main>
      <h1>DolphinClient</h1>
      <p className="tagline">Mehr FPS, weniger Aufwand — für Minecraft 26.1.</p>
      <p className="honest">
        Vorkonfigurierte Performance-Mods, Cosmetics und ein Launcher mit
        Microsoft-Login. Ehrlich gesagt: Die FPS kommen aus erstklassigen
        Open-Source-Mods — wir bündeln sie bequem an einem Ort und bringen
        Cosmetics &amp; Community dazu.
      </p>

      <div className="cta">
        <Link className="btn" href="/download">
          Herunterladen
        </Link>
        <Link className="btn ghost" href="/account">
          Account
        </Link>
      </div>

      <section className="features">
        <div>
          <h3>Performance</h3>
          <p>Sodium &amp; Co. vorkonfiguriert — flüssig out of the box.</p>
        </div>
        <div>
          <h3>Module an/aus</h3>
          <p>Deaktivierte Module kosten keine Leistung.</p>
        </div>
        <div>
          <h3>Cosmetics</h3>
          <p>Capes und mehr — sichtbar für andere DolphinClient-Nutzer.</p>
        </div>
        <div>
          <h3>Ein Klick</h3>
          <p>Launcher mit Microsoft-Login und Auto-Update.</p>
        </div>
      </section>
    </main>
  );
}
