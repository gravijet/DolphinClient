import type { ReactNode } from "react";
import type { Metadata, Viewport } from "next";
import Link from "next/link";
import { Space_Grotesk, Inter } from "next/font/google";
import "./globals.css";

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
  metadataBase: new URL("https://example.invalid"),
  title: {
    default: "DolphinClient — Mehr FPS für Minecraft 26.1",
    template: "%s · DolphinClient",
  },
  description:
    "Performance-orientierter Minecraft-Client für 26.1: vorkonfigurierte Mods (Sodium & Co.), Cosmetics und ein Launcher mit Microsoft-Login und Auto-Update.",
  applicationName: "DolphinClient",
  openGraph: {
    title: "DolphinClient — Mehr FPS für Minecraft 26.1",
    description:
      "Vorkonfigurierte Performance-Mods, Cosmetics und ein Ein-Klick-Launcher mit Auto-Update.",
    url: "https://example.invalid",
    siteName: "DolphinClient",
    locale: "de_DE",
    type: "website",
  },
  icons: { icon: [{ url: "/favicon.svg", type: "image/svg+xml" }] },
};

export const viewport: Viewport = { themeColor: "#050b14" };

function Logo() {
  return (
    <svg viewBox="0 0 48 48" aria-hidden="true" xmlns="http://www.w3.org/2000/svg">
      <defs>
        <linearGradient id="dcLogo" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#38e1c4" />
          <stop offset="0.5" stopColor="#4ac6ff" />
          <stop offset="1" stopColor="#7c8bff" />
        </linearGradient>
      </defs>
      <rect width="48" height="48" rx="13" fill="url(#dcLogo)" />
      <path
        fill="#ffffff"
        d="M13 33c7-9 13-15 21-18-4 6-7 11-7 17-3-2-9-2-13 2-.4-.4-1-.6-1-1Z"
      />
      <path
        d="M11 36c5-3 9 2 14-1 5-3 9 2 13-1"
        fill="none"
        stroke="#ffffff"
        strokeOpacity="0.85"
        strokeWidth="2.4"
        strokeLinecap="round"
      />
    </svg>
  );
}

export default function RootLayout({ children }: { children: ReactNode }) {
  const year = new Date().getFullYear();
  return (
    <html lang="de" className={`${spaceGrotesk.variable} ${inter.variable}`}>
      <body>
        <nav className="nav">
          <Link href="/" className="nav__brand">
            <Logo />
            DolphinClient
          </Link>
          <span className="nav__spacer" />
          <div className="nav__links">
            <Link href="/" className="nav__hide-sm">
              Start
            </Link>
            <Link href="/download">Download</Link>
            <Link href="/account">Account</Link>
            <Link href="/download" className="nav__cta">
              Herunterladen
            </Link>
          </div>
        </nav>

        {children}

        <footer>
          <span>© {year} DolphinClient · Minecraft 26.1</span>
          <span>
            Nicht mit Mojang oder Microsoft verbunden. Minecraft ist eine Marke
            von Mojang Synergies AB.
          </span>
        </footer>
      </body>
    </html>
  );
}
