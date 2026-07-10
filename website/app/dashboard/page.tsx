import type { Metadata } from "next";
import Dashboard from "./Dashboard";

export const metadata: Metadata = {
  title: "Dashboard",
  description:
    "Dein Live-Dashboard: verbindet sich direkt mit dem laufenden DolphinClient-Launcher und zeigt Konto, Version, Spielzeit, Cosmetics und Einstellungen in Echtzeit.",
};

export default function DashboardPage() {
  return (
    <main>
      <section className="hero" style={{ paddingBottom: "0.5rem" }}>
        <span className="hero__eyebrow">
          <span className="dot" /> Dashboard · Live mit dem Launcher verbunden
        </span>
        <h1>
          Dein <span className="glow">Live-Dashboard</span>.
        </h1>
        <p className="tagline">
          Diese Seite liest direkt aus deinem laufenden Launcher — aktives Konto,
          Client-Version, Spielzeit und Einstellungen in Echtzeit. Kein zweites
          Login, kein Server dazwischen.
        </p>
      </section>

      <Dashboard />
    </main>
  );
}
