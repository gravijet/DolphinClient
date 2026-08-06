"use client";

// Renders the version-history list. It paints immediately from the baked-in
// fallback (`CHANGES`, from changelog.json at build time) and then, on the
// client, replaces it with the always-current `/downloads/changelog.json` —
// the same runtime-published file the launcher's manifest lives beside. This is
// what lets a new release appear on the site without rebuilding the static
// export: only the JSON is republished.

import { useEffect, useState } from "react";
import Reveal from "./Reveal";
import { CHANGES, CHANGELOG_URL, type ChangeEntry } from "../changelog/data";

export default function ChangelogFeed({
  limit,
  delayStep = 45,
}: {
  /** Show only the newest N entries (the /download snapshot). */
  limit?: number;
  /** Per-item reveal stagger in ms. */
  delayStep?: number;
}) {
  const [entries, setEntries] = useState<ChangeEntry[]>(CHANGES);

  useEffect(() => {
    let alive = true;
    fetch(CHANGELOG_URL, { cache: "no-store" })
      .then((r) => (r.ok ? r.json() : null))
      .then((data) => {
        if (alive && Array.isArray(data) && data.length) {
          setEntries(data as ChangeEntry[]);
        }
      })
      .catch(() => {
        /* keep the baked fallback */
      });
    return () => {
      alive = false;
    };
  }, []);

  const shown = limit ? entries.slice(0, limit) : entries;

  return (
    <>
      {shown.map((c, i) => (
        <Reveal key={c.v} variant="left" delay={Math.min(i, 8) * delayStep}>
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
    </>
  );
}
