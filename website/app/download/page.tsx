import type { Metadata } from "next";
import Link from "next/link";
import DownloadCards from "./DownloadCards";
import Reveal from "../components/Reveal";
import { CHANGES } from "../changelog/data";

export const metadata: Metadata = {
  title: "Download",
  description:
    "Download the DolphinClient launcher for Windows. Free, small and installed in seconds — then play Minecraft 26.1 with a native client.",
};

const REQS = [
  { k: "System", v: "Windows 10/11" },
  { k: "Account", v: "Microsoft account (Minecraft)" },
  { k: "Storage", v: "A few hundred MB free" },
  { k: "Price", v: "Free" },
];

// Only the most recent releases here; the full history lives on /changelog.
const RECENT = CHANGES.slice(0, 4);

const FAQ = [
  { q: "Is the download safe?", a: "Yes. Sign-in goes through Microsoft's official dialog, the game files come straight from Mojang, and your credentials stay on your PC. The files aren't code-signed yet, so Windows may show a notice on first run." },
  { q: "Does Windows warn on launch?", a: "It can. While the file isn't signed, Windows may show SmartScreen. Choose “More info” → “Run anyway” to start the launcher normally. Signing will follow." },
  { q: "Do I need to set anything up?", a: "No. Download, install, sign in with Microsoft, press Play — done. The launcher handles the rest in the background." },
  { q: "What gets downloaded?", a: "On first launch the client fetches the original game data from Mojang (you need a paid account) and the client itself. After that it's cached and you're ready instantly." },
  { q: "Which systems are supported?", a: "Windows 10/11 today, as an installer with shortcuts and automatic updates. macOS and Linux aren't available yet." },
  { q: "How do I install on Windows?", a: "Download the setup, double-click it — the launcher installs without admin rights, adds shortcuts and keeps itself up to date from then on." },
  { q: "Can I remove it again?", a: "Anytime. The launcher uninstalls like any other program and doesn't change your normal Minecraft." },
];

export default function DownloadPage() {
  return (
    <main>
      <section className="hero" style={{ paddingBottom: "1rem" }}>
        <span className="kicker">Download · free</span>
        <h1>
          Get <span className="accent">DolphinClient</span>.
        </h1>
        <p className="hero__lede" style={{ maxWidth: "44ch" }}>
          A small launcher, installed in seconds — then Minecraft 26.1 runs on a
          native client. For your system.
        </p>
        <p className="hero__note">
          All you need is a Microsoft account. The rest — fetching game data,
          signing in, staying current — the launcher handles for you.
        </p>
      </section>

      <DownloadCards />

      {/* requirements */}
      <Reveal as="section" className="sec-head">
        <span className="sec-head__idx">[ Requirements ]</span>
        <span className="kicker">What you need</span>
        <h2 className="sec-title">Quick check</h2>
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
        <span className="sec-head__idx" id="changelog">[ New ]</span>
        <span className="kicker">What's changed</span>
        <h2 className="sec-title">Latest updates</h2>
        <p className="sec-lede">
          A snapshot — the{" "}
          <Link href="/changelog" style={{ color: "var(--accent)" }}>full history</Link>{" "}
          lives on the changelog page.
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
          <Link className="btn ghost" href="/changelog">Full history</Link>
        </div>
      </section>

      {/* faq */}
      <Reveal as="section" className="sec-head" style={{ scrollMarginTop: "90px" }}>
        <span className="sec-head__idx" id="faq">[ Questions ]</span>
        <span className="kicker">Before you download</span>
        <h2 className="sec-title">Common questions</h2>
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
        Note: the files aren't code-signed yet, so Windows may briefly show a
        warning. <Link href="/">Back to home</Link>
      </p>
    </main>
  );
}
