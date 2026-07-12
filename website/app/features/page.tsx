import type { Metadata } from "next";
import Link from "next/link";
import Reveal from "../components/Reveal";
import Compare from "../components/Compare";
import Logo from "../components/Logo";

export const metadata: Metadata = {
  title: "Vorteile",
  description:
    "Warum DolphinClient? Mehr FPS, kürzere Ladezeiten, weniger Arbeitsspeicher — Seite an Seite mit normalem Minecraft. Dazu ein aufgeräumter Launcher mit Konten-Verwaltung.",
};

const s = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.7,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};
const I = {
  bolt: <svg viewBox="0 0 24 24" {...s}><path d="M13 2 4 14h7l-1 8 9-12h-7l1-8Z" /></svg>,
  timer: <svg viewBox="0 0 24 24" {...s}><circle cx="12" cy="13" r="8" /><path d="M12 13V9M9 2h6M18 6l1.5-1.5" /></svg>,
  feather: <svg viewBox="0 0 24 24" {...s}><path d="M20 4c-6 0-11 4-13 10l-3 6M20 4 8 16M13 9h4M9 13h4" /></svg>,
  users: <svg viewBox="0 0 24 24" {...s}><circle cx="9" cy="8" r="3.5" /><path d="M2.5 20c0-3.6 2.9-5.5 6.5-5.5S15.5 16.4 15.5 20M17 5a3.5 3.5 0 0 1 0 6.5M22 20c0-2.8-1.6-4.6-4-5.2" /></svg>,
  swap: <svg viewBox="0 0 24 24" {...s}><path d="M4 8h13l-3-3M20 16H7l3 3" /></svg>,
  shield: <svg viewBox="0 0 24 24" {...s}><path d="M12 3 5 6v5c0 4.2 2.8 7.6 7 9 4.2-1.4 7-4.8 7-9V6l-7-3Z" /><path d="m9.5 12 1.8 1.8L15 10" /></svg>,
  refresh: <svg viewBox="0 0 24 24" {...s}><path d="M21 12a9 9 0 1 1-2.6-6.4M21 3v5h-5" /></svg>,
  check: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round"><path d="m20 6-11 11-5-5" /></svg>,
};

const BENEFITS = [
  { icon: I.bolt, title: "Mehr FPS", body: "Deutlich flüssiger als normales Minecraft — direkt spürbar, ohne dass du irgendetwas einstellen musst." },
  { icon: I.timer, title: "Schneller Start", body: "Vom Doppelklick zur Welt in wenigen Sekunden. Kein langer Ladebildschirm, kein Warten." },
  { icon: I.feather, title: "Weniger Speicher", body: "Braucht rund die Hälfte des Arbeitsspeichers. Weniger Hitze, weniger Lüfterlärm, mehr Luft für alles andere." },
  { icon: I.users, title: "Mehrere Konten", body: "Beliebig viele Microsoft-Konten hinzufügen und mit einem Klick wechseln — alles an einem Ort." },
  { icon: I.swap, title: "Konten übernehmen", body: "Schon woanders angemeldet? Bestehende Konten von deinem PC mit einem Klick übernehmen — ohne neue Anmeldung." },
  { icon: I.shield, title: "Anmeldung, die hält", body: "Klappt eine Anmeldung mal nicht, wird sie automatisch aufgefrischt — und falls nötig, fragt der Launcher klar nach." },
  { icon: I.refresh, title: "Immer aktuell", body: "Launcher und Spiel halten sich von selbst auf dem neuesten Stand — auf Wunsch ganz ohne einen einzigen Klick." },
  { icon: I.timer, title: "Startet mit dem PC", body: "Optional öffnet sich DolphinClient direkt beim Anmelden — ein Handgriff weniger vor dem Spielen." },
];

const COMPARE: { label: string; us: string | boolean; them: string | boolean }[] = [
  { label: "Bilder pro Sekunde (FPS)", us: "bis 3× mehr", them: "normal" },
  { label: "Zeit bis spielbereit", us: "wenige Sekunden", them: "spürbar länger" },
  { label: "Arbeitsspeicher", us: "rund die Hälfte", them: "voller Verbrauch" },
  { label: "Installation", us: "eine kleine Datei", them: "mehrteilig" },
  { label: "Automatisch aktuell", us: true, them: false },
  { label: "Mehrere Konten verwalten", us: true, them: false },
  { label: "Konten aus anderen Launchern übernehmen", us: true, them: false },
  { label: "Echte 26.1-Server, gleiche Regeln", us: true, them: true },
];

const ROADMAP = [
  { tag: "Fertig", done: true, title: "Flüssiges Spiel & schneller Start", body: "Hohe FPS und kurze Ladezeiten — die Grundlage, auf der alles aufbaut." },
  { tag: "Fertig", done: true, title: "Komplettes Spielmenü", body: "Titelbildschirm, Optionen für Video, Steuerung, Chat und Ton, Pausemenü und ein Info-Overlay im Spiel." },
  { tag: "Fertig", done: true, title: "Mehrere Konten & Übernahme", body: "Konten hinzufügen, wechseln und entfernen — oder ein bestehendes Konto von einem anderen Launcher auf deinem PC übernehmen." },
  { tag: "Fertig", done: true, title: "Anmeldung, die hält", body: "Fehlgeschlagene Anmeldungen werden automatisch aufgefrischt; erst wenn das nicht reicht, fragt der Launcher klar nach." },
  { tag: "Fertig", done: true, title: "Updates & Autostart", body: "Launcher und Spiel bleiben von selbst aktuell — auf Wunsch ohne Klick — und öffnen sich optional beim Anmelden." },
  { tag: "Als Nächstes", done: false, title: "Cosmetics in der Welt", body: "Eigene Capes, die auch für andere Spieler sichtbar sind — sobald die Grundlage steht." },
];

function Cell({ value }: { value: string | boolean }) {
  if (value === true) return <span className="compare__yes">{I.check}</span>;
  if (value === false) return <span className="compare__no" aria-label="nein">—</span>;
  return <span>{value}</span>;
}

export default function FeaturesPage() {
  return (
    <main>
      <section className="hero" style={{ paddingBottom: "1rem" }}>
        <span className="kicker">Vorteile</span>
        <h1>
          Warum <span className="accent">DolphinClient?</span>
        </h1>
        <p className="hero__lede" style={{ maxWidth: "46ch" }}>
          Weil dasselbe Spiel plötzlich viel besser läuft. Kein Marketing-Nebel,
          keine Fachbegriffe — nur der ehrliche Vergleich und was du davon hast.
        </p>
      </section>

      {/* THE COMPARISON */}
      <Reveal as="section" className="sec-head" id="vergleich" style={{ scrollMarginTop: "90px" }}>
        <span className="sec-head__idx">[ Der Vergleich ]</span>
        <span className="kicker">Seite an Seite</span>
        <h2 className="sec-title">Gleicher PC, deutlicher Unterschied</h2>
        <p className="sec-lede">
          Dieselbe Welt, dieselben Server — einmal mit, einmal ohne DolphinClient.
        </p>
      </Reveal>
      <Compare />
      <p className="notice">
        Richtwerte aus eigenen Messungen auf typischer Hardware. Wie groß der
        Unterschied bei dir ausfällt, hängt von PC und Szene ab.
      </p>

      {/* BENEFITS */}
      <Reveal as="section" className="sec-head" id="vorteile" style={{ scrollMarginTop: "90px" }}>
        <span className="sec-head__idx">[ Was du bekommst ]</span>
        <span className="kicker">Sechs gute Gründe</span>
        <h2 className="sec-title">Alles, was den Alltag besser macht</h2>
      </Reveal>
      <section className="sheet">
        <div className="sheet__grid cols-3">
          {BENEFITS.map((f, i) => (
            <Reveal key={f.title} variant="up" delay={(i % 3) * 70}>
              <div className="cell">
                <span className="cell__icon">{f.icon}</span>
                <h3>{f.title}</h3>
                <p>{f.body}</p>
              </div>
            </Reveal>
          ))}
        </div>
      </section>

      {/* COMPARE TABLE */}
      <Reveal as="section" className="sec-head">
        <span className="sec-head__idx">[ Direkt gegenübergestellt ]</span>
        <span className="kicker">Zeile für Zeile</span>
        <h2 className="sec-title">DolphinClient und normales Minecraft</h2>
      </Reveal>
      <Reveal as="section" variant="up" className="compare-wrap">
        <table className="compare">
          <thead>
            <tr>
              <th />
              <th className="is-us">DolphinClient</th>
              <th>Normales Minecraft</th>
            </tr>
          </thead>
          <tbody>
            {COMPARE.map((row) => (
              <tr key={row.label}>
                <td className="compare__label">{row.label}</td>
                <td className="is-us"><Cell value={row.us} /></td>
                <td><Cell value={row.them} /></td>
              </tr>
            ))}
          </tbody>
        </table>
      </Reveal>

      {/* LAUNCHER */}
      <Reveal as="section" variant="up" className="split reverse" id="launcher" style={{ scrollMarginTop: "90px" }}>
        <div className="split__media">
          <div className="readout">
            <div className="readout__top">
              <Logo />
              <span>dein launcher</span>
              <span className="readout__dot" />
            </div>
            <div className="readout__rows" style={{ paddingTop: "18px" }}>
              <div className="readout__row"><span className="k">start</span><span className="l" /><span className="v good">in Sekunden</span></div>
              <div className="readout__row"><span className="k">konten</span><span className="l" /><span className="v">beliebig viele</span></div>
              <div className="readout__row"><span className="k">anmeldung</span><span className="l" /><span className="v good">hält & frischt auf</span></div>
              <div className="readout__row"><span className="k">updates</span><span className="l" /><span className="v good">automatisch</span></div>
              <div className="readout__row"><span className="k">autostart</span><span className="l" /><span className="v">optional</span></div>
            </div>
          </div>
        </div>
        <div>
          <span className="kicker">Der Launcher</span>
          <h3>Ein aufgeräumter Startpunkt</h3>
          <p>
            Der Launcher ist bewusst schlicht: ein kleines, schnelles Fenster,
            das sofort öffnet. Deine Konten und die wenigen Einstellungen, die
            wirklich zählen, liegen an einem Ort — kein Wühlen, kein Fachwissen.
          </p>
          <ul>
            <li><span className="mk">Konten</span><span>hinzufügen, wechseln, entfernen — oder bestehende übernehmen</span></li>
            <li><span className="mk">Anmeldung</span><span>frischt sich selbst auf; nur zur Not fragt der Launcher nach</span></li>
            <li><span className="mk">Updates</span><span>hält sich und das Spiel aktuell — auf Wunsch ohne Klick</span></li>
            <li><span className="mk">Start</span><span>öffnet sich optional gleich beim Anmelden am PC</span></li>
          </ul>
          <div className="cta">
            <Link className="btn" href="/download">Launcher laden</Link>
          </div>
        </div>
      </Reveal>

      {/* ROADMAP */}
      <Reveal as="section" className="sec-head" id="roadmap" style={{ scrollMarginTop: "90px" }}>
        <span className="sec-head__idx">[ Was noch kommt ]</span>
        <span className="kicker">Ehrlich & Schritt für Schritt</span>
        <h2 className="sec-title">Was fertig ist — und was folgt</h2>
        <p className="sec-lede">
          Ein Client auf dem Niveau der großen Namen ist viel Arbeit. Wir bauen
          sichtbar und in Etappen.
        </p>
      </Reveal>
      <section className="changelog">
        {ROADMAP.map((r) => (
          <Reveal key={r.title} variant="left">
            <div className={`change${r.done ? " is-current" : ""}`}>
              <div className="change__head">
                <span className="change__v">{r.title}</span>
                <span className={`pill ${r.done ? "on" : "aqua"}`}>{r.tag}</span>
              </div>
              <p style={{ margin: 0, color: "var(--muted)", fontSize: "0.95rem" }}>{r.body}</p>
            </div>
          </Reveal>
        ))}
      </section>

      {/* CTA */}
      <Reveal as="section" variant="zoom" className="cta-band">
        <span className="kicker">Selbst nachprüfen</span>
        <h2>Zahlen sind billig. Probier es aus.</h2>
        <p>Lade den Launcher und spür den Unterschied auf deinem eigenen PC — kostenlos und in unter einer Minute.</p>
        <div className="cta">
          <Link className="btn lg" href="/download">Kostenlos laden</Link>
          <Link className="btn ghost lg" href="/changelog">Versionsverlauf</Link>
        </div>
      </Reveal>
    </main>
  );
}
