"use client";

// Renders the version-history list. It paints immediately from the baked-in
// fallback (`CHANGES`, from changelog.json at build time) and then, on the
// client, replaces it with the always-current `/downloads/changelog.json` —
// the same runtime-published file the launcher's manifest lives beside. This is
// what lets a new release appear on the site without rebuilding the static
// export: only the JSON is republished.
//
// The full page is long — sixty releases of prose — so everything below the
// newest few is folded into a <details> and there is a filter box. Both matter
// most on a phone, where the unfolded page was several metres of scrolling.

import { useEffect, useMemo, useState } from "react";
import Reveal from "./Reveal";
import ShotGallery from "./ShotGallery";
import { CHANGES, CHANGELOG_URL, type ChangeEntry } from "../changelog/data";

/** Renders `code spans` written with backticks; everything else stays text. */
function Line({ text }: { text: string }) {
  const parts = text.split(/(`[^`]+`)/g);
  return (
    <>
      {parts.map((p, i) =>
        p.startsWith("`") && p.endsWith("`") && p.length > 2 ? (
          <code key={i}>{p.slice(1, -1)}</code>
        ) : (
          <span key={i}>{p}</span>
        ),
      )}
    </>
  );
}

function Body({ entry }: { entry: ChangeEntry }) {
  return (
    <>
      <ul>
        {entry.items.map((it) => (
          <li key={it}>
            <Line text={it} />
          </li>
        ))}
      </ul>
      {entry.shots?.length ? <ShotGallery version={entry.v} shots={entry.shots} /> : null}
    </>
  );
}

export default function ChangelogFeed({
  limit,
  delayStep = 45,
  /** Entries from this index on start folded (the full page only). */
  collapseFrom,
  /** Show the filter box above the list. */
  searchable = false,
}: {
  /** Show only the newest N entries (the /download snapshot). */
  limit?: number;
  /** Per-item reveal stagger in ms. */
  delayStep?: number;
  collapseFrom?: number;
  searchable?: boolean;
}) {
  const [entries, setEntries] = useState<ChangeEntry[]>(CHANGES);
  const [query, setQuery] = useState("");

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

  const q = query.trim().toLowerCase();
  const matched = useMemo(
    () =>
      q
        ? entries.filter(
            (c) =>
              c.v.toLowerCase().includes(q) ||
              c.date.toLowerCase().includes(q) ||
              c.items.some((i) => i.toLowerCase().includes(q)),
          )
        : entries,
    [entries, q],
  );

  const shown = limit ? matched.slice(0, limit) : matched;
  // While filtering, show every hit open — a fold would hide the answer.
  const foldFrom = q ? Infinity : (collapseFrom ?? Infinity);

  return (
    <>
      {searchable && (
        <div className="change-filter">
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter — a version, a word, a feature…"
            aria-label="Filter the changelog"
          />
          <span>
            {q
              ? `${matched.length} of ${entries.length} releases`
              : `${entries.length} releases`}
          </span>
        </div>
      )}

      {shown.length === 0 && (
        <p className="change-empty">Nothing matches “{query}”.</p>
      )}

      {shown.map((c, i) => {
        const current = i === 0 && !q;
        if (i >= foldFrom) {
          return (
            <details key={c.v} className="change is-folded">
              <summary>
                <span className="change__v">{c.v}</span>
                <span className="change__date">{c.date}</span>
                <span className="change__more">
                  {c.items.length} notes{c.shots?.length ? ` · ${c.shots.length} pictures` : ""}
                </span>
              </summary>
              <Body entry={c} />
            </details>
          );
        }
        return (
          <Reveal key={c.v} variant="left" delay={Math.min(i, 8) * delayStep}>
            <div className={`change${current ? " is-current" : ""}`}>
              <div className="change__head">
                <span className="change__v">{c.v}</span>
                <span className="change__date">{c.date}</span>
              </div>
              <Body entry={c} />
            </div>
          </Reveal>
        );
      })}
    </>
  );
}
