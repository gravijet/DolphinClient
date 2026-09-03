import type { Metadata } from "next";
import Link from "next/link";
import Reveal from "../components/Reveal";
import Logo from "../components/Logo";

export const metadata: Metadata = {
  title: "Features",
  description:
    "What DolphinClient is: a native client for Minecraft 26.1 with its own small launcher — one-click play, multiple accounts, account import, resilient sign-in and automatic updates.",
};

const s = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.7,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};
const I = {
  chip: <svg viewBox="0 0 24 24" {...s}><rect x="7" y="7" width="10" height="10" rx="2" /><path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3" /></svg>,
  timer: <svg viewBox="0 0 24 24" {...s}><circle cx="12" cy="13" r="8" /><path d="M12 13V9M9 2h6M18 6l1.5-1.5" /></svg>,
  feather: <svg viewBox="0 0 24 24" {...s}><path d="M20 4c-6 0-11 4-13 10l-3 6M20 4 8 16M13 9h4M9 13h4" /></svg>,
  users: <svg viewBox="0 0 24 24" {...s}><circle cx="9" cy="8" r="3.5" /><path d="M2.5 20c0-3.6 2.9-5.5 6.5-5.5S15.5 16.4 15.5 20M17 5a3.5 3.5 0 0 1 0 6.5M22 20c0-2.8-1.6-4.6-4-5.2" /></svg>,
  swap: <svg viewBox="0 0 24 24" {...s}><path d="M4 8h13l-3-3M20 16H7l3 3" /></svg>,
  shield: <svg viewBox="0 0 24 24" {...s}><path d="M12 3 5 6v5c0 4.2 2.8 7.6 7 9 4.2-1.4 7-4.8 7-9V6l-7-3Z" /><path d="m9.5 12 1.8 1.8L15 10" /></svg>,
  refresh: <svg viewBox="0 0 24 24" {...s}><path d="M21 12a9 9 0 1 1-2.6-6.4M21 3v5h-5" /></svg>,
  gamepad: <svg viewBox="0 0 24 24" {...s}><rect x="2" y="7" width="20" height="12" rx="6" /><path d="M7 10v4M5 12h4M15.5 10.5h.01M18.5 12.5h.01" /></svg>,
  ear: <svg viewBox="0 0 24 24" {...s}><path d="M8 13a4 4 0 0 1 4-4 3 3 0 0 1 3 3c0 2-2 2-2 4a2 2 0 0 0 2 2" /><path d="M9 17c-3-1-5-3.5-5-7a8 8 0 0 1 16 0c0 1.5-.5 2.5-1 3.5" /></svg>,
  compass: <svg viewBox="0 0 24 24" {...s}><circle cx="12" cy="12" r="9" /><path d="m15 9-2 6-4 2 2-6 4-2Z" /></svg>,
};

const FEATURES = [
  { icon: I.chip, title: "Native engine", body: "A real client for Minecraft 26.1, written in Rust with its own renderer — not a mod layered on top of Java." },
  { icon: I.timer, title: "Fast startup", body: "The launcher opens instantly, and Play takes you into the game without a long wait." },
  { icon: I.feather, title: "Light on your PC", body: "A small download and a lean client that leaves room for everything else you're running." },
  { icon: I.users, title: "Microsoft & offline profiles", body: "Switch paid Microsoft accounts or create Vanilla-compatible identities for servers that explicitly allow offline mode." },
  { icon: I.feather, title: "Private skins & capes", body: "Import a Vanilla-layout PNG per profile, choose Classic or Slim arms, and see it locally without uploading it anywhere." },
  { icon: I.swap, title: "Import accounts", body: "Already signed in elsewhere? Import existing accounts from other launchers on your PC — no new sign-in." },
  { icon: I.shield, title: "Sign-in that holds", body: "If a sign-in fails, it refreshes on its own; only when that isn't enough does the launcher ask you clearly." },
  { icon: I.refresh, title: "Automatic updates", body: "The launcher and the client keep themselves current — optionally without a single click." },
  { icon: I.timer, title: "Optional autostart", body: "If you want, DolphinClient opens when you sign in to your PC — one less step before you play." },
  { icon: I.compass, title: "Built for real vanilla parity", body: "Weather, mobs, villagers, redstone, containers, lighting, particles — rendered and simulated to match a real 26.1 server, not approximated." },
  { icon: I.gamepad, title: "Play with a controller", body: "Plug in a gamepad and go: analog movement and look, full menu navigation, and a dedicated Controller options tab — no keyboard required." },
  { icon: I.ear, title: "Built-in accessibility", body: "A narrator that reads menus and chat through your OS's own text-to-speech, a high-contrast block outline, and toggles for lightning flashes and darkness pulsing." },
];

const INCLUDED = [
  { title: "Native gameplay & fast startup", body: "The client renders and plays Minecraft 26.1 itself, and the launcher gets you in quickly." },
  { title: "Complete game menus", body: "Title screen, options for video, controls, chat and sound, a pause menu and an in-game info overlay." },
  { title: "Multiple accounts & import", body: "Add, switch and remove accounts — or import one from another launcher on your PC." },
  { title: "Offline profiles & private cosmetics", body: "Use deterministic Vanilla offline identities on compatible servers and local-only skins or capes per profile." },
  { title: "Windows, Linux & macOS", body: "Architecture-specific downloads and verified updates keep x64, ARM, Intel and Apple Silicon artifacts separate." },
  { title: "Sign-in that holds", body: "Failed sign-ins refresh automatically; only if that isn't enough does the launcher prompt you." },
  { title: "Updates & autostart", body: "The launcher and client stay current on their own — optionally without a click — and can open at sign-in." },
  { title: "Deep vanilla-parity rendering", body: "Real lighting, weather, water, particles, mob variants, villagers, containers and dozens of block entities — built to look and behave like the real thing." },
  { title: "Gamepad support", body: "Analog movement and camera look, full menu navigation with the d-pad and face buttons, and a Controller tab in Options with sensitivity and deadzone." },
  { title: "Accessibility options", body: "A system-text-to-speech narrator, a high-contrast selection outline, and toggles to soften lightning flashes and darkness pulsing." },
];

export default function FeaturesPage() {
  return (
    <main>
      <section className="hero" style={{ paddingBottom: "1rem" }}>
        <span className="kicker">Features</span>
        <h1>
          What is <span className="accent">DolphinClient?</span>
        </h1>
        <p className="hero__lede" style={{ maxWidth: "48ch" }}>
          A native client for Minecraft 26.1 with its own small launcher. No
          jargon — here's exactly what it does and what you get.
        </p>
      </section>

      {/* FEATURES */}
      <Reveal as="section" className="sec-head" id="features" style={{ scrollMarginTop: "90px" }}>
        <span className="sec-head__idx">[ What it does ]</span>
        <span className="kicker">Everything in one place</span>
        <h2 className="sec-title">Built to keep it simple</h2>
      </Reveal>
      <section className="sheet">
        <div className="sheet__grid cols-3">
          {FEATURES.map((f, i) => (
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

      {/* GAMEPLAY */}
      <Reveal as="section" variant="up" className="split" id="gameplay" style={{ scrollMarginTop: "90px" }}>
        <div>
          <span className="kicker">The client</span>
          <h3>As close to the real thing as a from-scratch client gets</h3>
          <p>
            DolphinClient isn't a launcher around the Java client — it's its own
            renderer and its own protocol implementation, built one vanilla
            system at a time: real lighting and weather, mob variants and
            villager trading, every container and workstation, redstone
            pistons, and a controller-friendly, accessible interface on top.
          </p>
          <ul>
            <li><span className="mk">Rendering</span><span>vanilla lighting, weather, flowing water, particles, shadows</span></li>
            <li><span className="mk">World</span><span>mob variants, villagers, redstone, containers, block entities</span></li>
            <li><span className="mk">Controller</span><span>analog movement/look and full menu navigation with a gamepad</span></li>
            <li><span className="mk">Accessibility</span><span>a narrator, high contrast, and toggles for flashes and pulsing</span></li>
          </ul>
          <div className="cta">
            <Link className="btn" href="/changelog">See the full changelog</Link>
          </div>
        </div>
        <div className="split__media">
          <div className="readout">
            <div className="readout__top">
              <Logo />
              <span>the client</span>
              <span className="readout__dot" />
            </div>
            <div className="readout__rows" style={{ paddingTop: "18px" }}>
              <div className="readout__row"><span className="k">renderer</span><span className="l" /><span className="v good">native, wgpu</span></div>
              <div className="readout__row"><span className="k">weather &amp; lighting</span><span className="l" /><span className="v good">vanilla-accurate</span></div>
              <div className="readout__row"><span className="k">mobs &amp; villagers</span><span className="l" /><span className="v good">variants, trading</span></div>
              <div className="readout__row"><span className="k">containers</span><span className="l" /><span className="v good">every workstation</span></div>
              <div className="readout__row"><span className="k">controller</span><span className="l" /><span className="v good">full support</span></div>
              <div className="readout__row"><span className="k">accessibility</span><span className="l" /><span className="v good">narrator, high contrast</span></div>
            </div>
          </div>
        </div>
      </Reveal>

      {/* LAUNCHER */}
      <Reveal as="section" variant="up" className="split reverse" id="launcher" style={{ scrollMarginTop: "90px" }}>
        <div className="split__media">
          <div className="readout">
            <div className="readout__top">
              <Logo />
              <span>the launcher</span>
              <span className="readout__dot" />
            </div>
            <div className="readout__rows" style={{ paddingTop: "18px" }}>
              <div className="readout__row"><span className="k">accounts</span><span className="l" /><span className="v">as many as you like</span></div>
              <div className="readout__row"><span className="k">offline</span><span className="l" /><span className="v good">Vanilla UUIDs</span></div>
              <div className="readout__row"><span className="k">cosmetics</span><span className="l" /><span className="v good">private per profile</span></div>
              <div className="readout__row"><span className="k">import</span><span className="l" /><span className="v good">from other launchers</span></div>
              <div className="readout__row"><span className="k">sign-in</span><span className="l" /><span className="v good">refreshes on its own</span></div>
              <div className="readout__row"><span className="k">updates</span><span className="l" /><span className="v good">automatic</span></div>
              <div className="readout__row"><span className="k">autostart</span><span className="l" /><span className="v">optional</span></div>
            </div>
          </div>
        </div>
        <div>
          <span className="kicker">The launcher</span>
          <h3>A tidy place to start</h3>
          <p>
            The launcher is deliberately simple: a small, fast window that opens
            instantly. Your accounts and the few settings that actually matter
            live in one place — no digging, no jargon.
          </p>
          <ul>
            <li><span className="mk">Accounts</span><span>add, switch, remove — or import existing ones</span></li>
            <li><span className="mk">Offline</span><span>local profiles for explicitly compatible servers</span></li>
            <li><span className="mk">Cosmetics</span><span>private skin, cape and arm model per profile</span></li>
            <li><span className="mk">Sign-in</span><span>refreshes itself; only asks you when it must</span></li>
            <li><span className="mk">Updates</span><span>keeps itself and the client current — optionally without a click</span></li>
            <li><span className="mk">Start</span><span>optionally opens when you sign in to your PC</span></li>
          </ul>
          <div className="cta">
            <Link className="btn" href="/download">Get the launcher</Link>
          </div>
        </div>
      </Reveal>

      {/* WHAT'S HERE TODAY */}
      <Reveal as="section" className="sec-head" id="today" style={{ scrollMarginTop: "90px" }}>
        <span className="sec-head__idx">[ What's here today ]</span>
        <span className="kicker">Shipped, not promised</span>
        <h2 className="sec-title">What's included right now</h2>
      </Reveal>
      <section className="changelog">
        {INCLUDED.map((r) => (
          <Reveal key={r.title} variant="left">
            <div className="change is-current">
              <div className="change__head">
                <span className="change__v">{r.title}</span>
              </div>
              <p style={{ margin: 0, color: "var(--muted)", fontSize: "0.95rem" }}>{r.body}</p>
            </div>
          </Reveal>
        ))}
      </section>

      {/* CTA */}
      <Reveal as="section" variant="zoom" className="cta-band">
        <span className="kicker">See for yourself</span>
        <h2>Try it on your own PC.</h2>
        <p>Download the launcher and play — free, and in under a minute.</p>
        <div className="cta">
          <Link className="btn lg" href="/download">Download free</Link>
          <Link className="btn ghost lg" href="/changelog">Version history</Link>
        </div>
      </Reveal>
    </main>
  );
}
