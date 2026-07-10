import type { Metadata } from "next";
import Link from "next/link";
import DownloadCards from "./DownloadCards";
import Reveal from "../components/Reveal";
import { CHANGES } from "../changelog/data";

export const metadata: Metadata = {
  title: "Herunterladen",
  description:
    "Lade den nativen DolphinClient-Launcher für Windows, macOS oder Linux herunter — startet den nativen Minecraft-26.1-Client in Rust.",
};

const REQS = [
  { k: "Betriebssystem", v: "Windows 10/11, macOS 12+, Linux" },
  { k: "Minecraft-Konto", v: "Gültiges Microsoft-Konto (kein Cracked)" },
  { k: "Grafik", v: "GPU mit Vulkan / Metal / DX12" },
  { k: "Java", v: "Nicht nötig — nativer Client" },
];

// Only the most recent releases here; the full history lives on /changelog.
const RECENT = CHANGES.slice(0, 4);

const FAQ = [
  { q: "Ist der Launcher sicher?", a: "Ja. Der Login läuft über den offiziellen Microsoft-Flow, die Texturen kommen direkt von Mojang, und dein Token liegt verschlüsselt in der OS-Keychain. Vor dem finalen Release werden die Binaries zusätzlich code-signiert." },
  { q: "Warnt Windows beim Start?", a: "Solange die Binaries noch nicht code-signiert sind, kann SmartScreen eine Warnung zeigen. Über „Weitere Informationen“ → „Trotzdem ausführen“ startet der Launcher. Code-Signing folgt." },
  { q: "Brauche ich Java?", a: "Nein. Der native DolphinClient ist die komplette Spiel-Engine in Rust und braucht kein Java und kein Fabric. Du brauchst nur eine halbwegs aktuelle GPU (Vulkan, Metal oder DirectX 12)." },
  { q: "Was lädt der Launcher herunter?", a: "Nur die Original-Texturen und -Modelle von Mojang (du musst das Spiel besitzen) und den nativen Client selbst. Danach rendert der Client die Welt eigenständig und verbindet sich direkt mit dem Server." },
  { q: "Auf welchen Systemen läuft es?", a: "Windows und Linux werden aktiv veröffentlicht: unter Windows als Installer (Setup.exe) mit Startmenü- und Desktop-Verknüpfung und automatischen Updates, unter Linux als natives Binary. macOS-Builds folgen." },
  { q: "Wie installiere ich unter Windows?", a: "Setup herunterladen, doppelklicken, fertig — der Launcher installiert sich nach %LOCALAPPDATA%\\Programs\\DolphinClient (kein Admin nötig), legt Verknüpfungen an und hält sich ab dann selbst aktuell." },
  { q: "Wie starte ich unter Linux?", a: "Die heruntergeladene Datei ausführbar machen (chmod +x DolphinClient-linux-x64) und starten. Der Launcher lädt den nativen Client nach und aktualisiert sich selbst gegen den Release-Feed." },
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
          die Original-Texturen von Mojang und den nativen DolphinClient, gibt
          deine Anmeldung direkt weiter und startet den Client — kein Java, kein
          Fabric. Er hält sich selbst per Auto-Update aktuell.
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

      {/* changelog (recent — full history on /changelog) */}
      <Reveal as="section" className="section-head left" style={{ scrollMarginTop: "90px" }}>
        <span className="eyebrow-sm" id="changelog">Changelog</span>
        <h2 className="section-title">Was neu ist</h2>
        <p className="section-sub" style={{ marginInline: 0 }}>
          Die letzten Veröffentlichungen — die{" "}
          <Link href="/changelog" style={{ color: "var(--cyan)" }}>
            vollständige Historie
          </Link>{" "}
          findest du auf der Changelog-Seite.
        </p>
      </Reveal>
      <section className="changelog">
        {RECENT.map((c, i) => (
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
        <div className="cta" style={{ marginTop: "0.4rem" }}>
          <Link className="btn ghost" href="/changelog">
            Vollständiges Changelog
          </Link>
        </div>
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
        Hinweis: Die Binaries sind noch nicht code-signiert — Windows SmartScreen
        bzw. macOS Gatekeeper zeigen daher eventuell eine Warnung. Auf macOS und
        Linux die heruntergeladene Datei ausführbar machen (<code>chmod +x</code>)
        und starten. <Link href="/">Zurück zur Startseite</Link>
      </p>
    </main>
  );
}
