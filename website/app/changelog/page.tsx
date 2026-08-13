import type { Metadata } from "next";
import Link from "next/link";
import Reveal from "../components/Reveal";
import ChangelogFeed from "../components/ChangelogFeed";

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
        <ChangelogFeed delayStep={45} collapseFrom={4} searchable />
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
