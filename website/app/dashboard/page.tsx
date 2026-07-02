import type { Metadata } from "next";
import Dashboard from "./Dashboard";

export const metadata: Metadata = {
  title: "Dashboard",
  description:
    "Dein DolphinClient-Dashboard: Übersicht, Cosmetics, Statistiken, Downloads und Einstellungen an einem Ort.",
};

export default function DashboardPage() {
  return (
    <main>
      <section className="hero" style={{ paddingBottom: "0.5rem" }}>
        <span className="hero__eyebrow">
          <span className="dot" /> Dashboard · Dein Konto
        </span>
        <h1>
          Willkommen zurück, <span className="glow">Spieler</span>.
        </h1>
        <p className="tagline">
          Alles über deinen DolphinClient — Cosmetics, Statistiken, Downloads und
          Einstellungen an einem Ort.
        </p>
      </section>

      <Dashboard />
    </main>
  );
}
