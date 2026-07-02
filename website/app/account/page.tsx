import type { Metadata } from "next";
import Link from "next/link";
import CosmeticsDashboard from "./CosmeticsDashboard";
import Reveal from "../components/Reveal";

export const metadata: Metadata = {
  title: "Account",
  description: "Verwalte deine DolphinClient-Cosmetics.",
};

export default function AccountPage() {
  return (
    <main>
      <section className="hero" style={{ paddingBottom: "0.5rem" }}>
        <span className="hero__eyebrow">
          <span className="dot" /> Account · Cosmetics
        </span>
        <h1>
          Deine <span className="glow">Cosmetics</span>.
        </h1>
        <p className="tagline">
          Wähle deine Cape — sichtbar für andere DolphinClient-Spieler.
        </p>
        <div className="cta">
          <Link className="btn" href="/dashboard">
            Zum vollen Dashboard
          </Link>
        </div>
      </section>

      <Reveal variant="up">
        <CosmeticsDashboard />
      </Reveal>

      <p className="honest" style={{ marginTop: "2rem" }}>
        Mehr Statistiken, Einstellungen und Downloads gibt es im{" "}
        <Link href="/dashboard">Dashboard</Link>. ·{" "}
        <Link href="/">Zurück zur Startseite</Link>
      </p>
    </main>
  );
}
