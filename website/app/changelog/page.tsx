import type { Metadata } from "next";
import Link from "next/link";
import Reveal from "../components/Reveal";
import { CHANGES } from "./data";

export const metadata: Metadata = {
  title: "Changelog",
  description:
    "The full version history of DolphinClient — from the first Electron launcher to the native Rust engine with its own launcher.",
};

export default function ChangelogPage() {
  return (
    <main>
      <section className="hero" style={{ paddingBottom: "1rem" }}>
        <span className="kicker">Changelog · version history</span>
        <h1>
          Every <span className="accent">version</span>,
          <br />
          in the open.
        </h1>
        <p className="hero__lede">
          An honest record of what landed in each release — newest first.
        </p>
        <div className="cta">
          <Link className="btn" href="/download">Download the latest</Link>
          <Link className="btn ghost" href="/features">See the features</Link>
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
        <span className="kicker">Always current</span>
        <h2>Never an outdated version.</h2>
        <p>
          The launcher keeps itself and the client up to date on every start —
          you don't have to do anything by hand.
        </p>
        <div className="cta">
          <Link className="btn lg" href="/download">Download free</Link>
        </div>
      </Reveal>
    </main>
  );
}
