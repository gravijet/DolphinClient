import type { Metadata } from "next";
import Link from "next/link";
import DownloadCards from "./DownloadCards";
import Reveal from "../components/Reveal";

export const metadata: Metadata = {
  title: "Herunterladen",
  description:
    "Lade den nativen DolphinClient-Launcher für Windows, macOS oder Linux herunter — für Minecraft 26.1.",
};

const REQS = [
  { k: "Betriebssystem", v: "Windows 10/11, macOS 12+, Linux" },
  { k: "Minecraft-Konto", v: "Gültiges Microsoft-Konto (kein Cracked)" },
  { k: "Java", v: "JDK 25 (für 26.1)" },
  { k: "RAM", v: "4 GB empfohlen (einstellbar)" },
];

const CHANGES = [
  {
    v: "v0.2.5",
    date: "Aktuell",
    items: [
      "Login-Seite öffnet automatisch — direkter Link mit bereits eingetragenem Code",
      "Ein Klick auf den Link → sofort bei Microsoft anmelden",
    ],
  },
  {
    v: "v0.2.4",
    date: "Ohne Azure-App",
    items: [
      "Login funktioniert ohne eigene Azure-App (offizielle Launcher-ID via login.live.com)",
      "Behebt den login_with_xbox-403 — keine Freischaltung mehr nötig",
    ],
  },
  {
    v: "v0.2.3",
    date: "Browser-Login",
    items: [
      "Direkter Microsoft-Login im Browser (für eigene Azure-Apps)",
    ],
  },
  {
    v: "v0.2.2",
    date: "Login-App",
    items: [
      "Neue Azure-App-Client-ID für den Microsoft-Login",
    ],
  },
  {
    v: "v0.2.1",
    date: "Login-Konfiguration",
    items: [
      "Login-Tenant per DOLPHIN_MS_TENANT einstellbar (Standard: persönliche Konten)",
      "Klarere Fehlermeldungen bei der Microsoft-Anmeldung",
    ],
  },
  {
    v: "v0.2.0",
    date: "Nativer Launcher",
    items: [
      "Neuer nativer Launcher in Rust (egui) — startet in unter 0,5 s, ~11 MB",
      "Microsoft-Login, Spielstart & Update-Check im Launcher integriert",
      "Website-Redesign: Dashboard, mehr Content, Effekte & Animationen",
    ],
  },
  {
    v: "v0.1.0",
    date: "Erstes Release",
    items: [
      "26.1-Client mit FPS-, Koordinaten-, Uhr- und Zoom-Modulen",
      "Erster Launcher (Electron) mit Microsoft-Login und Spielstart",
    ],
  },
  {
    v: "v0.0.1",
    date: "Fundament",
    items: [
      "Monorepo mit Client, Launcher, Backend und Website",
      "Cosmetics-API (Capes) mit Account-Dashboard",
    ],
  },
];

const FAQ = [
  { q: "Ist der Launcher sicher?", a: "Ja. Der Login läuft über den offiziellen Microsoft-Flow, Spieldateien kommen direkt von Mojang, und dein Token liegt verschlüsselt in der OS-Keychain. Vor dem finalen Release werden die Installer zusätzlich code-signiert." },
  { q: "Warnt Windows beim Start?", a: "Solange die Installer noch nicht code-signiert sind, kann SmartScreen eine Warnung zeigen. Über „Weitere Informationen“ → „Trotzdem ausführen“ startet die Installation. Code-Signing folgt." },
  { q: "Brauche ich Java selbst installiert?", a: "Für 26.1 wird JDK 25 benötigt. Setze bei Bedarf den Pfad in den Launcher-Einstellungen; andernfalls wird „java“ aus dem PATH genutzt." },
  { q: "Auf welchen Systemen läuft es?", a: "Windows ist der primäre Fokus (native .exe). macOS (.dmg) und Linux (.AppImage) werden ebenfalls gebaut." },
];

export default function DownloadPage() {
  return (
    <main>
      <section className="hero" style={{ paddingBottom: "1rem" }}>
        <span className="hero__eyebrow">
          <span className="dot" /> Download · Nativer Launcher
        </span>
        <h1>
          Hol dir <span className="glow">DolphinClient</span>.
        </h1>
        <p className="tagline">Der blitzschnelle native Launcher für Minecraft 26.1 — für dein System.</p>
        <p className="honest">
          Du brauchst ein gültiges Minecraft-Konto (Microsoft). Der Launcher lädt
          die Original-Spieldateien direkt von Mojang, richtet Fabric samt Mods
          ein und hält sich selbst per Auto-Update aktuell.
        </p>
      </section>

      <DownloadCards />

      {/* system requirements */}
      <Reveal as="section" className="section-head left">
        <span className="eyebrow-sm">Systemvoraussetzungen</span>
        <h2 className="section-title">Was du brauchst</h2>
      </Reveal>
      <section className="reqs">
        {REQS.map((r, i) => (
          <Reveal key={r.k} variant="up" delay={i * 60}>
            <div className="req">
              <b>{r.k}</b>
              <span>{r.v}</span>
            </div>
          </Reveal>
        ))}
      </section>

      {/* changelog */}
      <Reveal as="section" className="section-head left" style={{ scrollMarginTop: "90px" }}>
        <span className="eyebrow-sm" id="changelog">Changelog</span>
        <h2 className="section-title">Was neu ist</h2>
      </Reveal>
      <section className="changelog">
        {CHANGES.map((c, i) => (
          <Reveal key={c.v} variant="left" delay={i * 70}>
            <div className="change">
              <h4>
                {c.v} <span>{c.date}</span>
              </h4>
              <ul>
                {c.items.map((it) => (
                  <li key={it}>{it}</li>
                ))}
              </ul>
            </div>
          </Reveal>
        ))}
      </section>

      {/* faq */}
      <Reveal as="section" className="section-head" style={{ scrollMarginTop: "90px" }}>
        <span className="eyebrow-sm" id="faq">FAQ</span>
        <h2 className="section-title">Häufige Fragen</h2>
      </Reveal>
      <section className="faq">
        {FAQ.map((f, i) => (
          <Reveal key={f.q} variant="fade" delay={i * 50}>
            <details open={i === 0}>
              <summary>{f.q}</summary>
              <p>{f.a}</p>
            </details>
          </Reveal>
        ))}
      </section>

      <p className="honest" style={{ marginTop: "2rem" }}>
        Hinweis: Die Installer sind noch nicht code-signiert — Windows SmartScreen
        bzw. macOS Gatekeeper zeigen daher eventuell eine Warnung. Auf Linux das{" "}
        <code>.AppImage</code> ausführbar machen (<code>chmod +x</code>) und
        starten. <Link href="/">Zurück zur Startseite</Link>
      </p>
    </main>
  );
}
