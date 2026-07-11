import type { Metadata } from "next";
import Link from "next/link";
import DownloadCards from "./DownloadCards";
import Reveal from "../components/Reveal";
import { CHANGES } from "../changelog/data";

export const metadata: Metadata = {
  title: "Beziehen",
  description:
    "Lade den nativen DolphinClient-Launcher für Windows, macOS oder Linux — er startet die native Minecraft-26.1-Engine in Rust. Kein Java nötig.",
};

const REQS = [
  { k: "System", v: "Windows 10/11 · macOS 12+ · Linux" },
  { k: "Konto", v: "Gültiges Microsoft-Konto" },
  { k: "Grafik", v: "GPU mit Vulkan / Metal / DX12" },
  { k: "Java", v: "Nicht erforderlich" },
];

// Only the most recent releases here; the full history lives on /changelog.
const RECENT = CHANGES.slice(0, 4);

const FAQ = [
  { q: "Ist der Launcher sicher?", a: "Ja. Die Anmeldung läuft über den offiziellen Microsoft-Flow, die Assets kommen direkt von Mojang, und dein Token liegt verschlüsselt in der OS-Keychain. Vor dem finalen Release werden die Binaries zusätzlich code-signiert." },
  { q: "Warnt Windows beim Start?", a: "Solange die Binaries noch nicht code-signiert sind, kann SmartScreen eine Warnung zeigen. Über „Weitere Informationen“ → „Trotzdem ausführen“ startet der Launcher. Code-Signing folgt." },
  { q: "Brauche ich Java?", a: "Nein. Der native DolphinClient ist die komplette Engine in Rust und braucht kein Java und kein Fabric. Nötig ist nur eine halbwegs aktuelle GPU (Vulkan, Metal oder DirectX 12)." },
  { q: "Was lädt der Launcher herunter?", a: "Nur die Original-Texturen und -Modelle von Mojang (du musst das Spiel besitzen) und den nativen Client selbst. Danach rendert der Client die Welt eigenständig und verbindet direkt mit dem Server." },
  { q: "Auf welchen Systemen läuft es?", a: "Aktiv veröffentlicht wird für Windows 10/11: als Installer (Setup.exe) mit Start­menü- und Desktop-Verknüpfung und automatischen Updates. Der Client ist plattformunabhängig in Rust geschrieben; Linux- und macOS-Builds stellen wir bei Bedarf bereit." },
  { q: "Wie installiere ich unter Windows?", a: "Setup laden, doppelklicken, fertig — der Launcher installiert sich nach %LOCALAPPDATA%\\Programs\\DolphinClient (kein Admin nötig), legt Verknüpfungen an und hält sich ab dann selbst aktuell." },
  { q: "Wie starte ich unter Linux?", a: "Die geladene Datei ausführbar machen (chmod +x DolphinClient-linux-x64) und starten. Der Launcher zieht den nativen Client nach und aktualisiert sich selbst gegen den Release-Feed." },
];

export default function DownloadPage() {
  return (
    <main>
      <section className="hero" style={{ paddingBottom: "1rem" }}>
        <span className="kicker">Beziehen · Nativer Launcher</span>
        <h1>
          Hol dir <span className="accent">DolphinClient</span>.
        </h1>
        <p className="hero__lede">
          Der winzige native Launcher startet die Minecraft-26.1-Engine — für
          dein System, in Sekunden.
        </p>
        <p className="hero__note">
          Du brauchst ein gültiges Microsoft-Konto. Der Launcher lädt die
          Original-Assets von Mojang und den nativen Client, gibt deine Anmeldung
          direkt weiter und hält sich per Auto-Update aktuell. Kein Java, kein
          Fabric.
        </p>
      </section>

      <DownloadCards />

      {/* requirements */}
      <Reveal as="section" className="sec-head">
        <span className="sec-head__idx">[ Voraussetzungen ]</span>
        <span className="kicker">Was du brauchst</span>
        <h2 className="sec-title">Systemvoraussetzungen</h2>
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
        <span className="sec-head__idx" id="changelog">[ Verlauf ]</span>
        <span className="kicker">Was neu ist</span>
        <h2 className="sec-title">Die letzten Builds</h2>
        <p className="sec-lede">
          Ein Ausschnitt — den{" "}
          <Link href="/changelog" style={{ color: "var(--aqua)" }}>vollständigen Verlauf</Link>{" "}
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
        <div className="cta" style={{ marginTop: "0.4rem", paddingLeft: "2rem" }}>
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
        Hinweis: Die Binaries sind noch nicht code-signiert — Windows SmartScreen
        bzw. macOS Gatekeeper zeigen daher eventuell eine Warnung. Auf macOS und
        Linux die geladene Datei ausführbar machen (<code>chmod +x</code>) und
        starten. <Link href="/">Zurück zur Startseite</Link>
      </p>
    </main>
  );
}
