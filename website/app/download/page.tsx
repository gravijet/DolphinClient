import type { Metadata } from "next";
import Link from "next/link";
import DownloadCards from "./DownloadCards";

export const metadata: Metadata = {
  title: "Herunterladen",
  description:
    "Lade den DolphinClient-Launcher für Windows, macOS oder Linux herunter — für Minecraft 26.1.",
};

export default function DownloadPage() {
  return (
    <main>
      <section className="hero">
        <span className="hero__eyebrow">Download · Launcher</span>
        <h1>
          Hol dir <span className="glow">DolphinClient</span>.
        </h1>
        <p className="tagline">Der Launcher für Minecraft 26.1 — für dein System.</p>
        <p className="honest">
          Du brauchst ein gültiges Minecraft-Konto (Microsoft). Der Launcher lädt
          die Original-Spieldateien direkt von Mojang, richtet Fabric samt Mods
          ein und hält sich selbst per Auto-Update aktuell.
        </p>
      </section>

      <DownloadCards />

      <p className="honest" style={{ marginTop: "2rem" }}>
        Hinweis: Die Installer sind noch nicht code-signiert — Windows SmartScreen
        bzw. macOS Gatekeeper zeigen daher eventuell eine Warnung. Auf Linux das{" "}
        <code>.AppImage</code> ausführbar machen (<code>chmod +x</code>) und
        starten. <Link href="/">Zurück zur Startseite</Link>
      </p>
    </main>
  );
}
