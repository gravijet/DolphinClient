"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";
import Logo from "./Logo";

const LINKS = [
  { href: "/", label: "Start" },
  { href: "/features", label: "Features" },
  { href: "/download", label: "Download" },
  { href: "/changelog", label: "Changelog" },
  { href: "/dashboard", label: "Dashboard" },
];

export default function SiteNav() {
  const pathname = usePathname();
  const [scrolled, setScrolled] = useState(false);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 12);
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, []);

  // Menü bei Navigation schließen.
  useEffect(() => setOpen(false), [pathname]);

  const isActive = (href: string) =>
    href === "/" ? pathname === "/" : pathname.startsWith(href);

  return (
    <nav className={`nav${scrolled ? " nav--scrolled" : ""}`}>
      <Link href="/" className="nav__brand">
        <Logo animated />
        <span>
          Dolphin<span className="nav__brand-accent">Client</span>
        </span>
      </Link>

      <span className="nav__spacer" />

      <div className={`nav__links${open ? " is-open" : ""}`}>
        {LINKS.map((l) => (
          <Link
            key={l.href}
            href={l.href}
            className={isActive(l.href) ? "is-active" : ""}
          >
            {l.label}
          </Link>
        ))}
        <Link href="/download" className="nav__cta">
          Herunterladen
        </Link>
      </div>

      <button
        className={`nav__burger${open ? " is-open" : ""}`}
        aria-label="Menü umschalten"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <span />
        <span />
        <span />
      </button>
    </nav>
  );
}
