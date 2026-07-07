import type { Metadata } from "next";
import Link from "next/link";
import DownloadCards from "./DownloadCards";
import Reveal from "../components/Reveal";

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

const CHANGES = [
  {
    v: "v0.3.0",
    date: "Aktuell · Nativer Client",
    items: [
      "Der Launcher startet jetzt den nativen DolphinClient (Rust + wgpu) statt Java-Minecraft — schneller Start, sehr hohe FPS, wenig RAM",
      "Kein Java/JDK mehr nötig: der Launcher lädt nur noch die Original-Texturen und -Modelle von Mojang, der Rest steckt im Client",
      "Deine Login-Session wird direkt an den Client übergeben — kein zweiter Login",
      "Server in den Einstellungen setzbar (direkt beitreten) oder Verbindungsbildschirm im Client",
    ],
  },
  {
    v: "v0.2.7",
    date: "Java-Launcher · LWJGL-Fix",
    items: [
      "Spielstart-Fix: nur die zur CPU-Architektur passenden LWJGL-Natives werden geladen (behebt lwjgl.dll-Fehler auf x64 endgültig)",
      "Keine Arch-Kollision mehr zwischen natives-windows / -arm64 / -x86",
    ],
  },
  {
    v: "v0.2.6",
    date: "Spielstart",
    items: [
      "Spielstart-Fix: LWJGL-Natives korrekt auf den Classpath (behebt lwjgl.dll-Fehler)",
      "Java-Version wird geprüft (26.1 braucht JDK 25) + Start-Log unter .minecraft/",
      "Abstürze werden jetzt direkt im Launcher angezeigt",
    ],
  },
  {
    v: "v0.2.5",
    date: "Login-Komfort",
    items: [
      "Login-Seite öffnet automatisch — direkter Link mit bereits eingetragenem Code",
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
  { q: "Ist der Launcher sicher?", a: "Ja. Der Login läuft über den offiziellen Microsoft-Flow, die Texturen kommen direkt von Mojang, und dein Token liegt verschlüsselt in der OS-Keychain. Vor dem finalen Release werden die Binaries zusätzlich code-signiert." },
  { q: "Warnt Windows beim Start?", a: "Solange die Binaries noch nicht code-signiert sind, kann SmartScreen eine Warnung zeigen. Über „Weitere Informationen“ → „Trotzdem ausführen“ startet der Launcher. Code-Signing folgt." },
  { q: "Brauche ich Java?", a: "Nein. Der native DolphinClient ist die komplette Spiel-Engine in Rust und braucht kein Java und kein Fabric. Du brauchst nur eine halbwegs aktuelle GPU (Vulkan, Metal oder DirectX 12)." },
  { q: "Was lädt der Launcher herunter?", a: "Nur die Original-Texturen und -Modelle von Mojang (du musst das Spiel besitzen) und den nativen Client selbst. Danach rendert der Client die Welt eigenständig und verbindet sich direkt mit dem Server." },
  { q: "Auf welchen Systemen läuft es?", a: "Windows ist der primäre Fokus: ein richtiger Installer (Setup.exe) mit Startmenü- und Desktop-Verknüpfung und automatischen Updates. macOS und Linux werden ebenfalls gebaut." },
  { q: "Wie installiere ich unter Windows?", a: "Setup herunterladen, doppelklicken, fertig — der Launcher installiert sich nach %LOCALAPPDATA%\\Programs\\DolphinClient (kein Admin nötig), legt Verknüpfungen an und hält sich ab dann selbst aktuell." },
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
        Hinweis: Die Binaries sind noch nicht code-signiert — Windows SmartScreen
        bzw. macOS Gatekeeper zeigen daher eventuell eine Warnung. Auf macOS und
        Linux die heruntergeladene Datei ausführbar machen (<code>chmod +x</code>)
        und starten. <Link href="/">Zurück zur Startseite</Link>
      </p>
    </main>
  );
}
