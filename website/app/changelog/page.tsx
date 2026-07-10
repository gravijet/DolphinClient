import type { Metadata } from "next";
import Link from "next/link";
import Reveal from "../components/Reveal";
import { CHANGES } from "./data";

export const metadata: Metadata = {
  title: "Changelog",
  description:
    "Die vollständige Versionshistorie von DolphinClient — vom ersten Electron-Launcher bis zum nativen Rust-Client mit Live-Dashboard.",
};

export default function ChangelogPage() {
  return (
    <main>
      <section className="hero" style={{ paddingBottom: "1rem" }}>
        <span className="hero__eyebrow">
          <span className="dot" /> Changelog · Versionshistorie
        </span>
        <h1>
          Jede Version von <span className="glow">DolphinClient</span>
        </h1>
        <p className="tagline">
          Ehrlich dokumentiert: was in jeder Veröffentlichung dazugekommen ist —
          neueste zuerst.
        </p>
        <div className="cta">
          <Link className="btn" href="/download">
            Neueste Version laden
          </Link>
          <Link className="btn ghost" href="/features">
            Features ansehen
          </Link>
        </div>
      </section>

      <section className="changelog" style={{ maxWidth: 860, marginInline: "auto" }}>
        {CHANGES.map((c, i) => (
          <Reveal key={c.v} variant="left" delay={Math.min(i, 8) * 55}>
            <div className="change">
              <h4>
                {c.v} <span>{c.date}</span>
              </h4>
              <ul>
                {c.items.map((it) => (
                  <li key={it}>{it}</li>
                ))}
              </ul>
            </div>
          </Reveal>
        ))}
      </section>

      <Reveal as="section" variant="zoom" className="cta-band">
        <h2>Immer auf der neuesten Version</h2>
        <p>
          Der Launcher prüft bei jedem Start die Signatur des Clients und lädt
          automatisch die aktuellste Build nach — du musst nichts manuell
          aktualisieren.
        </p>
        <div className="cta">
          <Link className="btn lg" href="/download">
            Herunterladen
          </Link>
        </div>
      </Reveal>
    </main>
  );
}
