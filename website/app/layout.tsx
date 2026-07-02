import type { ReactNode } from "react";
import type { Metadata, Viewport } from "next";
import Link from "next/link";
import { Space_Grotesk, Inter } from "next/font/google";
import "./globals.css";
import SiteNav from "./components/SiteNav";
import BackgroundFX from "./components/BackgroundFX";
import Logo from "./components/Logo";

const spaceGrotesk = Space_Grotesk({
  subsets: ["latin"],
  weight: ["500", "600", "700"],
  variable: "--font-space-grotesk",
  display: "swap",
});
const inter = Inter({
  subsets: ["latin"],
  variable: "--font-inter",
  display: "swap",
});

export const metadata: Metadata = {
  metadataBase: new URL("https://dolphin.gravijet.net"),
  title: {
    default: "DolphinClient — Mehr FPS für Minecraft 26.1",
    template: "%s · DolphinClient",
  },
  description:
    "Performance-orientierter Minecraft-Client für 26.1: vorkonfigurierte Mods (Sodium & Co.), Cosmetics und ein blitzschneller nativer Launcher mit Microsoft-Login und Auto-Update.",
  applicationName: "DolphinClient",
  keywords: [
    "Minecraft",
    "Client",
    "26.1",
    "Performance",
    "FPS",
    "Sodium",
    "Fabric",
    "Launcher",
    "Cosmetics",
  ],
  openGraph: {
    title: "DolphinClient — Mehr FPS für Minecraft 26.1",
    description:
      "Vorkonfigurierte Performance-Mods, Cosmetics und ein blitzschneller nativer Ein-Klick-Launcher mit Auto-Update.",
    url: "https://dolphin.gravijet.net",
    siteName: "DolphinClient",
    locale: "de_DE",
    type: "website",
  },
  icons: { icon: [{ url: "/favicon.svg", type: "image/svg+xml" }] },
};

export const viewport: Viewport = { themeColor: "#050b14" };

const FOOTER_COLS = [
  {
    title: "Produkt",
    links: [
      { href: "/features", label: "Features" },
      { href: "/download", label: "Download" },
      { href: "/dashboard", label: "Dashboard" },
      { href: "/account", label: "Cosmetics" },
    ],
  },
  {
    title: "Client",
    links: [
      { href: "/features#performance", label: "Performance" },
      { href: "/features#modules", label: "Module & HUD" },
      { href: "/features#launcher", label: "Nativer Launcher" },
      { href: "/download#changelog", label: "Changelog" },
    ],
  },
  {
    title: "Ressourcen",
    links: [
      { href: "/download#faq", label: "FAQ" },
      { href: "/features#roadmap", label: "Roadmap" },
      { href: "mailto:gravijetbedwars@gmail.com", label: "Kontakt" },
    ],
  },
];

export default function RootLayout({ children }: { children: ReactNode }) {
  const year = new Date().getFullYear();
  return (
    <html lang="de" className={`${spaceGrotesk.variable} ${inter.variable}`}>
      <body>
        <BackgroundFX />
        <SiteNav />

        {children}

        <footer>
          <div className="footer__top wide">
            <div>
              <Link href="/" className="footer__brand">
                <Logo />
                DolphinClient
              </Link>
              <p className="footer__blurb">
                Mehr FPS, weniger Aufwand. Ein blitzschneller nativer Launcher,
                vorkonfigurierte Mods und Cosmetics für Minecraft 26.1.
              </p>
            </div>
            {FOOTER_COLS.map((col) => (
              <div key={col.title} className="footer__col">
                <h4>{col.title}</h4>
                {col.links.map((l) => (
                  <Link key={l.href} href={l.href}>
                    {l.label}
                  </Link>
                ))}
              </div>
            ))}
          </div>

          <div className="footer__bottom wide">
            <span>
              © {year} DolphinClient · Minecraft 26.1 · Nicht mit Mojang oder
              Microsoft verbunden. Minecraft ist eine Marke von Mojang Synergies AB.
            </span>
            <div className="footer__social">
              <a href="mailto:gravijetbedwars@gmail.com" aria-label="E-Mail">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <rect x="2" y="4" width="20" height="16" rx="3" />
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
