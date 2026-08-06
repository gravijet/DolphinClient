// Single source of truth for the version history, shared by the full
// /changelog page and the condensed changelog block on /download.
//
// The entries live in `changelog.json` (git-tracked). At build time this file
// re-exports them as the baked-in fallback seed; at runtime the pages fetch the
// live `/downloads/changelog.json` (published on every release WITHOUT a website
// rebuild), so a new release shows up on the site the moment its downloads go
// live — the static export no longer has to be rebuilt just to add an entry.

import raw from "./changelog.json";

export interface ChangeEntry {
  v: string;
  date: string;
  items: string[];
}

export const CHANGES: ChangeEntry[] = raw as ChangeEntry[];

// The runtime location the pages fetch for the always-current history.
export const CHANGELOG_URL = "/downloads/changelog.json";
