import Link from "next/link";
import Reveal from "./components/Reveal";
import ReleaseDashboard from "./components/ReleaseDashboard";

/* -------- soft line icons -------- */
const s = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.7,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};
const I = {
  chip: (
    <svg viewBox="0 0 24 24" {...s}>
      <rect x="7" y="7" width="10" height="10" rx="2" />
      <path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3" />
    </svg>
  ),
  cursor: (
    <svg viewBox="0 0 24 24" {...s}>
      <path d="M5 3l6 18 2.5-7.5L21 11 5 3Z" />
    </svg>
  ),
  users: (
    <svg viewBox="0 0 24 24" {...s}>
      <circle cx="9" cy="8" r="3.5" />
      <path d="M2.5 20c0-3.6 2.9-5.5 6.5-5.5S15.5 16.4 15.5 20M17 5a3.5 3.5 0 0 1 0 6.5M22 20c0-2.8-1.6-4.6-4-5.2" />
    </svg>
  ),
  refresh: (
    <svg viewBox="0 0 24 24" {...s}>
      <path d="M21 12a9 9 0 1 1-2.6-6.4M21 3v5h-5" />
    </svg>
  ),
};

const BENEFITS = [
  {
    icon: I.chip,
    title: "Native engine",
    body: "Built from the ground up in Rust with its own renderer — a real client for Minecraft 26.1, not a mod stacked on top of Java.",
  },
  {
    icon: I.cursor,
    title: "One click to play",
    body: "Open the launcher, choose a Microsoft or offline profile, press Play. It fetches everything it needs and drops you straight into the game.",
  },
  {
    icon: I.users,
    title: "Profiles & cosmetics",
    body: "Switch Microsoft and offline profiles in one click, then give each one a private local skin, cape and Classic or Slim model.",
  },
  {
    icon: I.refresh,
    title: "Stays up to date",
    body: "The launcher and the client keep themselves current — optionally without a single click — so you never chase downloads.",
  },
];

const STEPS = [
  { t: "Download", d: "Grab the small launcher and install it in a few seconds. No extra software." },
  { t: "Choose a profile", d: "Sign in through Microsoft's own dialog, or create a local identity for a server that explicitly supports offline mode." },
  { t: "Play", d: "Press Play — the launcher fetches the game and keeps everything up to date on its own." },
];

const FAQ = [
  {
    q: "Is this cheating?",
    a: "No. DolphinClient plays normal Minecraft 26.1 on real servers — the same rules, blocks and sounds. It's a different client for the same game, not a hack.",
  },
  {
    q: "Does it cost anything?",
    a: "No. The launcher is free. Online-mode servers require a Minecraft-owning Microsoft account; deliberately configured offline-mode servers can use a local profile.",
  },
  {
    q: "Which systems are supported?",
    a: "Windows 10/11, Linux and macOS. Intel and ARM artifacts are published separately and all platforms use verified automatic updates.",
  },
  {
    q: "Do I stay up to date?",
    a: "Yes. The launcher keeps itself and the client current — you never have to download or update anything by hand.",
  },
];

export default function HomePage() {
  return (
    <main>
      {/* ---------- HERO ---------- */}
      <section className="hero">
        <div className="hero__grid">
          <div>
            <span className="kicker">For Minecraft 26.1</span>
            <h1>
              Minecraft,
              <br />
              <span className="accent">native.</span>
            </h1>
            <p className="hero__lede">
              A native client for Minecraft 26.1 with its own small launcher.
              Pick your profile and play — one click.
            </p>
            <p className="hero__note">
              Same servers, same rules. Multiple accounts, automatic updates,
              nothing to configure.
            </p>

            <div className="cta">
              <Link className="btn lg" href="/download">Download free</Link>
              <Link className="btn ghost lg" href="/features">See the features</Link>
            </div>

            <div className="hero__meta">
              <div className="m"><b>Free</b><span>always</span></div>
              <div className="m"><b>3 OS</b><span>Win · Linux · Mac</span></div>
              <div className="m"><b>Auto</b><span>updates</span></div>
            </div>
          </div>

          {/* live release dashboard, read from the real manifest */}
          <ReleaseDashboard />
        </div>
      </section>

      {/* ---------- BENEFITS ---------- */}
      <Reveal as="section" className="sec-head">
        <span className="sec-head__idx">[ 01 · What you get ]</span>
        <span className="kicker">Why DolphinClient</span>
        <h2 className="sec-title">A real client, kept simple</h2>
      </Reveal>
      <section className="sheet">
        <div className="sheet__grid">
          {BENEFITS.map((f, i) => (
            <Reveal key={f.title} variant="up" delay={(i % 2) * 80}>
              <div className="cell">
                <span className="cell__idx">{String(i + 1).padStart(2, "0")}</span>
                <span className="cell__icon">{f.icon}</span>
                <h3>{f.title}</h3>
                <p>{f.body}</p>
              </div>
            </Reveal>
          ))}
        </div>
      </section>

      {/* ---------- STEPS ---------- */}
      <Reveal as="section" className="sec-head">
        <span className="sec-head__idx">[ 02 · Getting started ]</span>
        <span className="kicker">In under a minute</span>
        <h2 className="sec-title">From download to playing</h2>
        <p className="sec-lede">Three steps, no prior knowledge needed.</p>
      </Reveal>
      <section className="steps">
        {STEPS.map((st, i) => (
          <Reveal key={st.t} variant="up" delay={i * 90}>
            <div className="step">
              <span className="step__n">Step {String(i + 1).padStart(2, "0")}</span>
              <h3>{st.t}</h3>
              <p>{st.d}</p>
            </div>
          </Reveal>
        ))}
      </section>

      {/* ---------- FAQ ---------- */}
      <Reveal as="section" className="sec-head">
        <span className="sec-head__idx">[ 03 · Questions ]</span>
        <span className="kicker">Short & honest</span>
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

      {/* ---------- CTA ---------- */}
      <Reveal as="section" variant="zoom" className="cta-band">
        <span className="kicker">Ready?</span>
        <h2>Get DolphinClient.</h2>
        <p>Download, sign in, play — in under a minute. Free, and you can remove it anytime.</p>
        <div className="cta">
          <Link className="btn lg" href="/download">Download free</Link>
          <Link className="btn ghost lg" href="/changelog">Version history</Link>
        </div>
      </Reveal>
    </main>
  );
}
