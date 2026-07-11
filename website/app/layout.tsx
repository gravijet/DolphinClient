import type { ReactNode } from "react";
import type { Metadata, Viewport } from "next";
import Link from "next/link";
import { Space_Grotesk, Inter, JetBrains_Mono } from "next/font/google";
import "./globals.css";
import SiteNav from "./components/SiteNav";
import BackgroundFX from "./components/BackgroundFX";
import Logo from "./components/Logo";

// Space Grotesk for display — a technical, slightly quirky grotesk that reads
// "engineering tool", not "generic SaaS". Inter for running text. JetBrains
// Mono for the spec labels, version tags and metadata that give the whole site
// its instrument-panel character.
const display = Space_Grotesk({
  subsets: ["latin"],
  weight: ["500", "600", "700"],
  variable: "--font-display",
  display: "swap",
});
const inter = Inter({
  subsets: ["latin"],
  variable: "--font-body",
  display: "swap",
});
const mono = JetBrains_Mono({
  subsets: ["latin"],
  weight: ["400", "500", "600"],
  variable: "--font-mono",
  display: "swap",
});

export const metadata: Metadata = {
  metadataBase: new URL("https://dolphinclient.de"),
  title: {
    default: "DolphinClient — Native Minecraft-Engine in Rust",
    template: "%s — DolphinClient",
  },
  description:
    "DolphinClient ist keine Mod und kein Modpack, sondern eine eigenständige Minecraft-26.1-Engine in Rust: eigener wgpu-Renderer, eigenes Netzwerk-Protokoll, echte Mojang-Assets. Dazu ein winziger nativer Launcher mit Microsoft-Login und ein Dashboard, das live am laufenden Launcher hängt. Ohne Java, ohne Fabric.",
  applicationName: "DolphinClient",
  keywords: [
    "Minecraft",
    "Client",
    "26.1",
    "Rust",
    "wgpu",
    "Engine",
    "nativ",
    "FPS",
    "Launcher",
    "Dashboard",
  ],
  openGraph: {
    title: "DolphinClient — Native Minecraft-Engine in Rust",
    description:
      "Eine eigenständige Minecraft-26.1-Engine in Rust — eigener Renderer, echte Vanilla-Assets, ein nativer Launcher und ein Live-Dashboard. Kein Java.",
    url: "https://dolphinclient.de",
    siteName: "DolphinClient",
    locale: "de_DE",
    type: "website",
  },
  icons: { icon: [{ url: "/favicon.png", type: "image/png" }] },
};

export const viewport: Viewport = { themeColor: "#05070d" };

const FOOTER_COLS = [
  {
    title: "Navigieren",
    links: [
      { href: "/features", label: "Technik" },
      { href: "/download", label: "Beziehen" },
      { href: "/changelog", label: "Verlauf" },
      { href: "/dashboard", label: "Dashboard" },
    ],
  },
  {
    title: "Technik",
    links: [
      { href: "/features#engine", label: "Engine" },
      { href: "/features#hud", label: "HUD & Optionen" },
      { href: "/features#launcher", label: "Launcher" },
      { href: "/features#roadmap", label: "Roadmap" },
    ],
  },
  {
    title: "Kontakt",
    links: [
      { href: "/download#faq", label: "Fragen & Antworten" },
      { href: "mailto:gravijetbedwars@gmail.com", label: "E-Mail schreiben" },
      { href: "https://github.com/gravijet", label: "GitHub" },
    ],
  },
];

export default function RootLayout({ children }: { children: ReactNode }) {
  const year = new Date().getFullYear();
  return (
    <html
      lang="de"
      className={`${display.variable} ${inter.variable} ${mono.variable}`}
    >
      <body>
        <BackgroundFX />
        <SiteNav />

        {children}

        <footer className="foot">
          <div className="foot__top wide">
            <div className="foot__brandcol">
              <Link href="/" className="foot__brand">
                <Logo />
                <span>
                  Dolphin<span className="foot__brand-accent">Client</span>
                </span>
              </Link>
              <p className="foot__blurb">
                Eine eigenständige Minecraft-26.1-Engine in Rust. Kein Java, kein
                Fabric — nur ein winziger Launcher und ein Client, der die Welt
                selbst rendert.
              </p>
              <p className="foot__coord">52.37°N / 4.90°E — gebaut aus dem Meer</p>
            </div>
            {FOOTER_COLS.map((col) => (
              <div key={col.title} className="foot__col">
                <h4>{col.title}</h4>
                {col.links.map((l) => (
                  <Link key={l.href} href={l.href}>
                    {l.label}
                  </Link>
                ))}
              </div>
            ))}
          </div>

          <div className="foot__bottom wide">
            <span>
              © {year} DolphinClient — Minecraft 26.1. Ein unabhängiges Projekt,
              nicht mit Mojang oder Microsoft verbunden. „Minecraft“ ist eine
              Marke von Mojang Synergies AB.
            </span>
            <div className="foot__social">
              <a href="mailto:gravijetbedwars@gmail.com" aria-label="E-Mail">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
                  <rect x="2" y="4" width="20" height="16" rx="2.5" />
                  <path d="m3 6 9 7 9-7" />
                </svg>
              </a>
              <a href="https://github.com/gravijet" aria-label="GitHub">
                <svg viewBox="0 0 24 24" fill="currentColor">
                  <path d="M12 2a10 10 0 0 0-3.16 19.49c.5.09.68-.22.68-.48v-1.7c-2.78.6-3.37-1.34-3.37-1.34-.45-1.16-1.11-1.47-1.11-1.47-.9-.62.07-.6.07-.6 1 .07 1.53 1.03 1.53 1.03.9 1.53 2.36 1.09 2.94.83.09-.65.35-1.09.63-1.34-2.22-.25-4.55-1.11-4.55-4.94 0-1.09.39-1.98 1.03-2.68-.1-.26-.45-1.27.1-2.65 0 0 .84-.27 2.75 1.02a9.5 9.5 0 0 1 5 0c1.91-1.29 2.75-1.02 2.75-1.02.55 1.38.2 2.39.1 2.65.64.7 1.03 1.59 1.03 2.68 0 3.84-2.34 4.68-4.57 4.93.36.31.68.92.68 1.85v2.74c0 .27.18.58.69.48A10 10 0 0 0 12 2Z" />
                </svg>
              </a>
            </div>
          </div>
        </footer>
      </body>
    </html>
  );
}
