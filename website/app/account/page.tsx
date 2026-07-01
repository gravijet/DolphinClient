import type { Metadata } from "next";
import Link from "next/link";
import CosmeticsDashboard from "./CosmeticsDashboard";

export const metadata: Metadata = {
  title: "Account",
  description: "Verwalte deine DolphinClient-Cosmetics.",
};

export default function AccountPage() {
  return (
    <main>
      <section className="hero">
        <span className="hero__eyebrow">Account · Cosmetics</span>
        <h1>
          Deine <span className="glow">Cosmetics</span>.
        </h1>
        <p className="tagline">
          Wähle deine Cape — sichtbar für andere DolphinClient-Spieler.
        </p>
      </section>

      <CosmeticsDashboard />

      <p className="honest" style={{ marginTop: "2rem" }}>
        <Link href="/">Zurück zur Startseite</Link>
      </p>
    </main>
  );
}
