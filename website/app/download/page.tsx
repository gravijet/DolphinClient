import type { Metadata } from "next";
import Link from "next/link";
import DownloadCards from "./DownloadCards";
import Reveal from "../components/Reveal";
import { CHANGES } from "../changelog/data";

export const metadata: Metadata = {
  title: "Download",
  description:
    "Lade den DolphinClient-Launcher für Windows, macOS oder Linux. Kostenlos, klein und in Sekunden installiert — dann spielst du Minecraft 26.1 spürbar schneller.",
};

const REQS = [
  { k: "System", v: "Windows 10/11 · macOS · Linux" },
  { k: "Konto", v: "Microsoft-Konto (Minecraft)" },
  { k: "Speicher", v: "Wenige hundert MB frei" },
  { k: "Preis", v: "Kostenlos" },
];

// Only the most recent releases here; the full history lives on /changelog.
const RECENT = CHANGES.slice(0, 4);

const FAQ = [
  { q: "Ist der Download sicher?", a: "Ja. Die Anmeldung läuft über den offiziellen Microsoft-Dialog, die Spieldaten kommen direkt von Mojang, und deine Zugangsdaten bleiben geschützt auf deinem PC. Vor dem großen Release werden die Dateien zusätzlich offiziell signiert." },
  { q: "Warnt Windows beim Start?", a: "Das kann vorkommen. Solange die Datei noch nicht signiert ist, zeigt Windows eventuell einen Hinweis. Über „Weitere Informationen“ → „Trotzdem ausführen“ startest du den Launcher ganz normal. Die Signatur folgt." },
  { q: "Muss ich irgendetwas einrichten?", a: "Nein. Laden, installieren, mit Microsoft anmelden, auf „Spielen“ klicken — fertig. Alles Weitere erledigt der Launcher im Hintergrund." },
  { q: "Was wird heruntergeladen?", a: "Beim ersten Start holt der Launcher die Original-Spieldaten von Mojang (dafür brauchst du ein gekauftes Konto) und das Spiel selbst. Danach ist alles gespeichert und du bist sofort startklar." },
  { q: "Auf welchen Systemen läuft es?", a: "Aktiv angeboten wird Windows 10/11: als bequemer Installer mit Verknüpfungen und automatischen Updates. macOS- und Linux-Versionen stellen wir bei Bedarf bereit." },
  { q: "Wie installiere ich unter Windows?", a: "Setup laden, doppelklicken, fertig — der Launcher installiert sich ohne Admin-Rechte, legt Verknüpfungen an und hält sich ab dann selbst aktuell." },
  { q: "Kann ich es wieder entfernen?", a: "Jederzeit. Der Launcher lässt sich wie jedes andere Programm deinstallieren und verändert dein normales Minecraft nicht." },
];

export default function DownloadPage() {
  return (
    <main>
      <section className="hero" style={{ paddingBottom: "1rem" }}>
        <span className="kicker">Download · kostenlos</span>
        <h1>
          Hol dir <span className="accent">DolphinClient</span>.
        </h1>
        <p className="hero__lede" style={{ maxWidth: "44ch" }}>
          Ein kleiner Launcher, in Sekunden installiert — und Minecraft 26.1 läuft
          spürbar schneller. Für dein System.
        </p>
        <p className="hero__note">
          Du brauchst nur ein Microsoft-Konto. Den Rest — Spieldaten laden,
          anmelden, aktuell halten — übernimmt der Launcher automatisch.
        </p>
      </section>

      <DownloadCards />

      {/* requirements */}
      <Reveal as="section" className="sec-head">
        <span className="sec-head__idx">[ Voraussetzungen ]</span>
        <span className="kicker">Was du brauchst</span>
        <h2 className="sec-title">Kurz gecheckt</h2>
      </Reveal>
      <section className="reqs">
        {REQS.map((r, i) => (
          <Reveal key={r.k} variant="up" delay={i * 55}>
            <div className="req">
              <b>{r.k}</b>
              <span>{r.v}</span>
            </div>
          </Reveal>
        ))}
      </section>

      {/* changelog (recent) */}
      <Reveal as="section" className="sec-head" style={{ scrollMarginTop: "90px" }}>
        <span className="sec-head__idx" id="changelog">[ Neu ]</span>
        <span className="kicker">Was sich getan hat</span>
        <h2 className="sec-title">Die letzten Updates</h2>
        <p className="sec-lede">
          Ein Ausschnitt — den{" "}
          <Link href="/changelog" style={{ color: "var(--accent)" }}>vollständigen Verlauf</Link>{" "}
          findest du auf der Verlaufsseite.
        </p>
      </Reveal>
      <section className="changelog">
        {RECENT.map((c, i) => (
          <Reveal key={c.v} variant="left" delay={i * 55}>
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
        <div className="cta" style={{ marginTop: "0.4rem" }}>
          <Link className="btn ghost" href="/changelog">Vollständiger Verlauf</Link>
        </div>
      </section>

      {/* faq */}
      <Reveal as="section" className="sec-head" style={{ scrollMarginTop: "90px" }}>
        <span className="sec-head__idx" id="faq">[ Fragen ]</span>
        <span className="kicker">Vor dem Download</span>
        <h2 className="sec-title">Häufige Fragen</h2>
      </Reveal>
      <section className="faq">
        {FAQ.map((f, i) => (
          <Reveal key={f.q} variant="fade" delay={i * 40}>
            <details open={i === 0}>
              <summary><span className="q-idx">{String(i + 1).padStart(2, "0")}</span>{f.q}</summary>
              <p>{f.a}</p>
            </details>
          </Reveal>
        ))}
      </section>

      <p className="notice">
        Hinweis: Die Dateien sind noch nicht offiziell signiert — Windows bzw.
        macOS zeigen daher eventuell kurz eine Warnung. Auf macOS und Linux die
        geladene Datei ausführbar machen (<code>chmod +x</code>) und starten.{" "}
        <Link href="/">Zurück zur Startseite</Link>
      </p>
    </main>
  );
}
