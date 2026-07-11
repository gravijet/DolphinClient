import type { Metadata } from "next";
import Link from "next/link";
import Reveal from "../components/Reveal";
import { CHANGES } from "./data";

export const metadata: Metadata = {
  title: "Verlauf",
  description:
    "Die vollständige Versionshistorie von DolphinClient — vom ersten Electron-Launcher bis zur nativen Rust-Engine mit Live-Dashboard.",
};

export default function ChangelogPage() {
  return (
    <main>
      <section className="hero" style={{ paddingBottom: "1rem" }}>
        <span className="kicker">Verlauf · Versionshistorie</span>
        <h1>
          Jede <span className="accent">Version</span>,
          <br />
          nachvollziehbar.
        </h1>
        <p className="hero__lede">
          Ehrlich dokumentiert: was in jeder Veröffentlichung dazugekommen ist —
          neueste zuerst.
        </p>
        <div className="cta">
          <Link className="btn" href="/download">Neueste Version laden</Link>
          <Link className="btn ghost" href="/features">Vorteile ansehen</Link>
        </div>
      </section>

      <section className="changelog" style={{ marginTop: "2.5rem" }}>
        {CHANGES.map((c, i) => (
          <Reveal key={c.v} variant="left" delay={Math.min(i, 8) * 45}>
            <div className={`change${i === 0 ? " is-current" : ""}`}>
              <div className="change__head">
                <span className="change__v">{c.v}</span>
                <span className="change__date">{c.date}</span>
              </div>
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
        <span className="kicker">Immer aktuell</span>
        <h2>Nie wieder eine veraltete Version.</h2>
        <p>
          Der Launcher hält sich und das Spiel bei jedem Start automatisch auf dem
          neuesten Stand — du musst nichts manuell tun.
        </p>
        <div className="cta">
          <Link className="btn lg" href="/download">Kostenlos laden</Link>
        </div>
      </Reveal>
    </main>
  );
}
