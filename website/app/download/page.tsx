import type { Metadata } from "next";
import Link from "next/link";
import DownloadCards from "./DownloadCards";
import Reveal from "../components/Reveal";
import ChangelogFeed from "../components/ChangelogFeed";

export const metadata: Metadata = {
  title: "Download",
  description:
    "Download the DolphinClient launcher for Windows, Linux or macOS and play Minecraft 26.1 with the native client.",
};

const REQS = [
  { k: "System", v: "Windows 10/11, Linux or macOS" },
  { k: "Account", v: "Microsoft, or offline profile on compatible servers" },
  { k: "Storage", v: "A few hundred MB free" },
  { k: "Price", v: "Free" },
];

const FAQ = [
  { q: "Is the download safe?", a: "Yes. Sign-in goes through Microsoft's official dialog, the game files come straight from Mojang, and your credentials stay on your PC. The files aren't code-signed yet, so Windows may show a notice on first run." },
  { q: "Does Windows warn on launch?", a: "It can. While the file isn't signed, Windows may show SmartScreen. Choose “More info” → “Run anyway” to start the launcher normally. Signing will follow." },
  { q: "Do I need to set anything up?", a: "No. Download, sign in with Microsoft or create an offline profile for a compatible server, then press Play. The launcher handles the client and updates." },
  { q: "What gets downloaded?", a: "On first launch the launcher fetches the original game assets from Mojang and the native client. Those files are cached for later launches." },
  { q: "Which systems are supported?", a: "Windows 10/11, Linux and macOS are supported. Intel and ARM downloads are kept separate so the launcher and game always match your CPU." },
  { q: "How do I install on Windows?", a: "Download the setup, double-click it — the launcher installs without admin rights, adds shortcuts and keeps itself up to date from then on." },
  { q: "Can I use my own skin or cape?", a: "Yes. The Cosmetics page imports a Vanilla-layout PNG per profile. It is rendered only on your device and never uploaded or presented as an official Mojang cosmetic." },
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
          Use Microsoft for normal online servers, or a local profile where a
          server explicitly allows offline mode. Downloads and updates are automatic.
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
        <ChangelogFeed limit={4} delayStep={55} />
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
