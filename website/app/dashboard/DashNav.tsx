"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";

const TABS: { href: string; label: string }[] = [
  { href: "/dashboard", label: "Overview" },
  { href: "/dashboard/profile", label: "Profile" },
  { href: "/dashboard/security", label: "Security" },
];

/** The sub-nav shared by every /dashboard* page, so switching between
 * overview/profile/security never loses the surrounding dashboard chrome. */
export default function DashNav() {
  const pathname = usePathname();
  return (
    <nav className="dash-tabs" aria-label="Account sections">
      {TABS.map((t) => (
        <Link key={t.href} href={t.href} className={pathname === t.href ? "is-on" : undefined}>
          {t.label}
        </Link>
      ))}
    </nav>
  );
}
