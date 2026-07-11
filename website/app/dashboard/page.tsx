import type { Metadata } from "next";
import Dashboard from "./Dashboard";

export const metadata: Metadata = {
  title: "Dashboard",
  description:
    "Dein Live-Dashboard: verbindet sich lokal mit dem laufenden DolphinClient-Launcher und zeigt Konto, Version, Spielzeit, Cosmetics und Einstellungen in Echtzeit.",
};

export default function DashboardPage() {
  return (
    <main>
      <section className="hero" style={{ paddingBottom: "1rem" }}>
        <span className="kicker">Dashboard · 127.0.0.1 · live</span>
        <h1>
          Dein <span className="accent">Live-Dashboard</span>.
        </h1>
        <p className="hero__lede">
          Diese Seite liest direkt aus deinem laufenden Launcher — aktives Konto,
          Client-Version, Spielzeit und Einstellungen in Echtzeit. Kein zweiter
          Login, kein Server dazwischen, keine erfundenen Zahlen.
        </p>
      </section>

      <Dashboard />
    </main>
  );
}
