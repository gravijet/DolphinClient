// Single source of truth for the version history, shared by the full
// /changelog page and the condensed changelog block on /download.
//
// The entries live in `changelog.json` (git-tracked). At build time this file
// re-exports them as the baked-in fallback seed; at runtime the pages fetch the
// live `/downloads/changelog.json` (published on every release WITHOUT a website
// rebuild), so a new release shows up on the site the moment its downloads go
// live — the static export no longer has to be rebuilt just to add an entry.

import raw from "./changelog.json";

/** One screenshot shown with a release. */
export interface ChangeShot {
  /** File name inside the release's shot folder, or an absolute path/URL. */
  src: string;
  /** What the picture shows — also the alt text. */
  alt: string;
}

export interface ChangeEntry {
  v: string;
  date: string;
  items: string[];
  /** Screenshots of that release, straight out of the client's own renderer. */
  shots?: ChangeShot[];
}

export const CHANGES: ChangeEntry[] = raw as ChangeEntry[];

// The runtime location the pages fetch for the always-current history.
export const CHANGELOG_URL = "/downloads/changelog.json";

// Where a release's screenshots are published (same runtime folder as the
// manifest and the changelog itself — no website rebuild to add pictures).
export const SHOTS_BASE = "/downloads/shots";

/** Absolute URL of a shot: absolute entries pass through, names are resolved
 *  inside that release's folder (`/downloads/shots/v0.60.0/<name>`). */
export function shotUrl(version: string, src: string): string {
  if (/^(https?:)?\//.test(src)) return src;
  return `${SHOTS_BASE}/${version}/${src}`;
}
