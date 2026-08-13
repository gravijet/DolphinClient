"use client";

// The changelog editor inside the admin portal: publish a release, rewrite one,
// reorder them, delete one, and manage the screenshots that go with it.
//
// It talks to deploy/admin-api.mjs at /admin-data/api/… — behind the same
// Cloudflare Access gate as the rest of the portal, so the browser's existing
// session is the only credential involved and nothing here holds a secret.
//
// What it edits is the LIVE file the website fetches (downloads/changelog.json),
// so a save is visible on /changelog immediately, with no rebuild and no
// deploy. `release.sh` pulls that file back into the repository before adding
// its own entry (deploy/sync-changelog.mjs), so an edit made here is never
// overwritten by the next release.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

const API = "/admin-data/api";

interface Shot {
  src: string;
  alt: string;
}
interface Entry {
  v: string;
  date: string;
  items: string[];
  shots?: Shot[];
}
interface DiskShot {
  src: string;
  bytes: number;
  at: string;
}
interface Backup {
  file: string;
  bytes: number;
  at?: string;
  reason?: string;
  by?: string;
}

/** The bullets are edited as one bullet per line — the same shape as the
 *  changelog files a release is built from. */
const toText = (items: string[]) => items.join("\n");
const fromText = (text: string) =>
  text
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);

const stripCurrent = (date: string) => date.replace(/^Current · /, "");
const kb = (n: number) => (n < 1024 ? `${n} B` : n < 1024 * 1024 ? `${Math.round(n / 1024)} kB` : `${(n / 1048576).toFixed(1)} MB`);

/** A file name the API will accept: lower case, no spaces, image suffix. */
function safeName(name: string) {
  const cleaned = name
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, "-")
    .replace(/^-+/, "")
    .slice(0, 80);
  return /\.(png|jpe?g|webp|gif)$/.test(cleaned) ? cleaned : `${cleaned || "shot"}.png`;
}

export default function ChangelogAdmin() {
  const [entries, setEntries] = useState<Entry[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState<Entry | null>(null);
  const [isNew, setIsNew] = useState(false);
  const [disk, setDisk] = useState<DiskShot[]>([]);
  const [backups, setBackups] = useState<Backup[]>([]);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<{ kind: "ok" | "err"; text: string } | null>(null);
  const [showBackups, setShowBackups] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);

  const say = (kind: "ok" | "err", text: string) => setNote({ kind, text });

  const api = useCallback(async (path: string, init?: RequestInit) => {
    const res = await fetch(`${API}${path}`, { cache: "no-store", ...init });
    const text = await res.text();
    let body: unknown = null;
    try {
      body = text ? JSON.parse(text) : null;
    } catch {
      /* an HTML error page from nginx */
    }
    if (!res.ok) {
      const msg = (body as { error?: string })?.error;
      if (res.status === 403 || res.status === 401) {
        throw new Error("Your session has expired — reload the page to sign in again.");
      }
      throw new Error(msg || `HTTP ${res.status}`);
    }
    return body as Record<string, unknown>;
  }, []);

  const loadList = useCallback(async () => {
    try {
      const r = await api("/changelog");
      setEntries((r.entries as Entry[]) ?? []);
    } catch (e) {
      say("err", (e as Error).message);
      setEntries([]);
    }
  }, [api]);

  useEffect(() => {
    void loadList();
  }, [loadList]);


  // Screenshots actually present on disk for the entry being edited.
  const loadDisk = useCallback(
    async (version: string) => {
      if (!/^v?\d+\.\d+\.\d+$/.test(version)) {
        setDisk([]);
        return;
      }
      try {
        const r = await api(`/shots/${version.replace(/^v/, "")}`);
        setDisk((r.files as DiskShot[]) ?? []);
      } catch {
        setDisk([]);
      }
    },
    [api],
  );

  const pick = useCallback(
    (entry: Entry) => {
      setSelected(entry.v);
      setIsNew(false);
      setDraft({ ...entry, date: stripCurrent(entry.date), shots: entry.shots ? [...entry.shots] : [] });
      setNote(null);
      void loadDisk(entry.v);
    },
    [loadDisk],
  );

  // Land on the newest release rather than an empty pane.
  useEffect(() => {
    if (entries?.length && !draft && !isNew) pick(entries[0]);
  }, [entries, draft, isNew, pick]);

  const startNew = () => {
    setSelected(null);
    setIsNew(true);
    setDraft({ v: "", date: "", items: [], shots: [] });
    setDisk([]);
    setNote(null);
  };

  const save = async () => {
    if (!draft) return;
    setBusy(true);
    try {
      const payload: Entry = {
        v: draft.v.trim(),
        date: draft.date.trim(),
        items: draft.items,
        shots: draft.shots?.length ? draft.shots : undefined,
      };
      const r = isNew
        ? await api("/changelog", { method: "POST", body: JSON.stringify(payload) })
        : await api(`/changelog/${(selected ?? draft.v).replace(/^v/, "")}`, {
            method: "PUT",
            body: JSON.stringify(payload),
          });
      const list = (r.entries as Entry[]) ?? [];
      setEntries(list);
      const saved = (r.entry as Entry) ?? list.find((e) => e.v.replace(/^v/, "") === payload.v.replace(/^v/, ""));
      if (saved) pick(saved);
      say("ok", isNew ? `Published ${saved?.v ?? payload.v} — it is live on /changelog now.` : `Saved ${saved?.v ?? payload.v}.`);
    } catch (e) {
      say("err", (e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const remove = async (entry: Entry) => {
    if (!window.confirm(`Delete ${entry.v} from the published changelog?\n\nA backup is kept, so this can be undone.`)) return;
    setBusy(true);
    try {
      const r = await api(`/changelog/${entry.v.replace(/^v/, "")}`, { method: "DELETE" });
      setEntries((r.entries as Entry[]) ?? []);
      setDraft(null);
      setSelected(null);
      say("ok", `Deleted ${entry.v}.`);
    } catch (e) {
      say("err", (e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const move = async (index: number, by: number) => {
    if (!entries) return;
    const next = [...entries];
    const to = index + by;
    if (to < 0 || to >= next.length) return;
    [next[index], next[to]] = [next[to], next[index]];
    setBusy(true);
    try {
      const r = await api("/changelog", { method: "PUT", body: JSON.stringify({ entries: next }) });
      setEntries((r.entries as Entry[]) ?? next);
      say("ok", "Order saved.");
    } catch (e) {
      say("err", (e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const upload = async (files: FileList | null) => {
    if (!files?.length || !draft) return;
    const version = draft.v.trim().replace(/^v/, "");
    if (!/^\d+\.\d+\.\d+$/.test(version)) {
      say("err", "Give the entry a version first (e.g. 0.61.0) — that is the folder the pictures go into.");
      return;
    }
    setBusy(true);
    try {
      for (const file of Array.from(files)) {
        const name = safeName(file.name);
        const r = await api(`/shots/${version}/${name}`, {
          method: "POST",
          body: await file.arrayBuffer(),
          headers: { "content-type": file.type || "application/octet-stream" },
        });
        setDisk((r.files as DiskShot[]) ?? []);
        setDraft((d) =>
          d && d.shots?.some((s) => s.src === name)
            ? d
            : d && { ...d, shots: [...(d.shots ?? []), { src: name, alt: "" }] },
        );
      }
      say("ok", "Uploaded. Give each picture a caption, then save the entry.");
    } catch (e) {
      say("err", (e as Error).message);
    } finally {
      setBusy(false);
      if (fileInput.current) fileInput.current.value = "";
    }
  };

  const deleteShot = async (name: string) => {
    if (!draft) return;
    const version = draft.v.trim().replace(/^v/, "");
    if (!window.confirm(`Delete the picture ${name} from the server?`)) return;
    setBusy(true);
    try {
      const r = await api(`/shots/${version}/${name}`, { method: "DELETE" });
      setDisk((r.files as DiskShot[]) ?? []);
      if (r.entries) setEntries(r.entries as Entry[]);
      setDraft((d) => d && { ...d, shots: (d.shots ?? []).filter((s) => s.src !== name) });
      say("ok", `Deleted ${name}.`);
    } catch (e) {
      say("err", (e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const loadBackups = async () => {
    try {
      const r = await api("/history");
      setBackups((r.backups as Backup[]) ?? []);
      setShowBackups(true);
    } catch (e) {
      say("err", (e as Error).message);
    }
  };

  const restore = async (file: string) => {
    if (!window.confirm(`Put this version of the changelog back?\n\n${file}`)) return;
    setBusy(true);
    try {
      const r = await api(`/restore/${file}`, { method: "POST" });
      setEntries((r.entries as Entry[]) ?? []);
      setDraft(null);
      setSelected(null);
      say("ok", "Restored.");
    } catch (e) {
      say("err", (e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const attached = useMemo(() => new Set((draft?.shots ?? []).map((s) => s.src)), [draft]);
  const version = draft?.v.trim().replace(/^v/, "") ?? "";
  const wordCount = draft ? draft.items.join(" ").split(/\s+/).filter(Boolean).length : 0;

  return (
    <section className="adm-panel cl">
      <div className="cl__bar">
        <h2>Changelog</h2>
        <p className="adm-note">
          Edits the published <code>downloads/changelog.json</code> directly — a save is live on{" "}
          <a href="/changelog" target="_blank" rel="noreferrer">
            /changelog
          </a>{" "}
          straight away, no rebuild. Every write keeps a backup.
        </p>
        <div className="cl__bar-actions">
          <button type="button" className="btn sm" onClick={startNew} disabled={busy}>
            New release
          </button>
          <button type="button" className="btn ghost sm" onClick={loadBackups} disabled={busy}>
            Backups
          </button>
        </div>
      </div>

      {note && <div className={`cl__note ${note.kind}`}>{note.text}</div>}

      {showBackups && (
        <div className="cl__backups">
          <div className="cl__backups-head">
            <b>Backups</b>
            <button type="button" className="btn ghost sm" onClick={() => setShowBackups(false)}>
              Close
            </button>
          </div>
          {backups.length === 0 && <p className="adm-empty">No backups yet.</p>}
          <ul>
            {backups.map((b) => (
              <li key={b.file}>
                <span className="cl__backup-when">{b.at ? new Date(b.at).toLocaleString("en-GB") : b.file}</span>
                <span className="cl__backup-why">
                  {b.reason ?? "—"}
                  {b.by ? ` · ${b.by}` : ""}
                </span>
                <span className="cl__backup-size">{kb(b.bytes)}</span>
                <button type="button" className="btn ghost sm" onClick={() => restore(b.file)} disabled={busy}>
                  Restore
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="cl__cols">
        <ol className="cl__list">
          {entries === null && <li className="adm-empty">Loading…</li>}
          {entries?.length === 0 && <li className="adm-empty">No entries.</li>}
          {entries?.map((e, i) => (
            <li key={e.v} className={e.v === selected ? "is-on" : undefined}>
              <button type="button" className="cl__pick" onClick={() => pick(e)}>
                <b>{e.v}</b>
                <span>{stripCurrent(e.date)}</span>
                <small>
                  {e.items.length} {e.items.length === 1 ? "line" : "lines"}
                  {e.shots?.length
                    ? ` · ${e.shots.length} ${e.shots.length === 1 ? "picture" : "pictures"}`
                    : ""}
                </small>
              </button>
              <div className="cl__rowbtns">
                <button type="button" aria-label={`Move ${e.v} up`} onClick={() => move(i, -1)} disabled={busy || i === 0}>
                  ↑
                </button>
                <button
                  type="button"
                  aria-label={`Move ${e.v} down`}
                  onClick={() => move(i, 1)}
                  disabled={busy || i === (entries?.length ?? 1) - 1}
                >
                  ↓
                </button>
                <button type="button" aria-label={`Delete ${e.v}`} className="danger" onClick={() => remove(e)} disabled={busy}>
                  ✕
                </button>
              </div>
            </li>
          ))}
        </ol>

        <div className="cl__edit">
          {!draft && <p className="adm-empty">Pick a release on the left, or start a new one.</p>}

          {draft && (
            <>
              <div className="cl__fields">
                <label>
                  <span>Version</span>
                  <input
                    value={draft.v}
                    onChange={(e) => setDraft({ ...draft, v: e.target.value })}
                    placeholder="0.61.0"
                    inputMode="decimal"
                    disabled={busy}
                  />
                </label>
                <label className="grow">
                  <span>Headline</span>
                  <input
                    value={draft.date}
                    onChange={(e) => setDraft({ ...draft, date: e.target.value })}
                    placeholder="What this release is about"
                    disabled={busy}
                  />
                </label>
              </div>
              <p className="cl__hint">
                The newest entry is marked “Current · …” automatically — don&apos;t type that yourself.
              </p>

              <label className="cl__body">
                <span>
                  Body — one bullet per line ({draft.items.length} {draft.items.length === 1 ? "bullet" : "bullets"},{" "}
                  {wordCount} words)
                </span>
                <textarea
                  value={toText(draft.items)}
                  onChange={(e) => setDraft({ ...draft, items: fromText(e.target.value) })}
                  rows={14}
                  spellCheck
                  disabled={busy}
                  placeholder="One paragraph per line. Blank lines are ignored."
                />
              </label>

              <div className="cl__shots">
                <div className="cl__shots-head">
                  <b>Screenshots</b>
                  <input
                    ref={fileInput}
                    type="file"
                    accept="image/png,image/jpeg,image/webp,image/gif"
                    multiple
                    onChange={(e) => upload(e.target.files)}
                    disabled={busy || !/^\d+\.\d+\.\d+$/.test(version)}
                  />
                </div>

                {disk.length === 0 && (
                  <p className="adm-empty">
                    {/^\d+\.\d+\.\d+$/.test(version)
                      ? "No pictures uploaded for this release yet."
                      : "Set a version to upload pictures."}
                  </p>
                )}

                <div className="cl__shotgrid">
                  {disk.map((f) => {
                    const shot = draft.shots?.find((s) => s.src === f.src);
                    const on = attached.has(f.src);
                    return (
                      <div key={f.src} className={`cl__shot${on ? " is-on" : ""}`}>
                        {/* eslint-disable-next-line @next/next/no-img-element */}
                        <img src={`/downloads/shots/v${version}/${f.src}`} alt={f.src} loading="lazy" />
                        <label className="cl__shot-on">
                          <input
                            type="checkbox"
                            checked={on}
                            onChange={(e) =>
                              setDraft({
                                ...draft,
                                shots: e.target.checked
                                  ? [...(draft.shots ?? []), { src: f.src, alt: "" }]
                                  : (draft.shots ?? []).filter((s) => s.src !== f.src),
                              })
                            }
                            disabled={busy}
                          />
                          <span>Show in the changelog</span>
                        </label>
                        <input
                          className="cl__shot-alt"
                          value={shot?.alt ?? ""}
                          onChange={(e) =>
                            setDraft({
                              ...draft,
                              shots: (draft.shots ?? []).map((s) =>
                                s.src === f.src ? { ...s, alt: e.target.value } : s,
                              ),
                            })
                          }
                          placeholder="Caption — what the picture shows"
                          disabled={busy || !on}
                        />
                        <div className="cl__shot-foot">
                          <span>
                            {f.src} · {kb(f.bytes)}
                          </span>
                          <button type="button" className="danger" onClick={() => deleteShot(f.src)} disabled={busy}>
                            Delete
                          </button>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>

              <div className="cl__actions">
                <button type="button" className="btn" onClick={save} disabled={busy || !draft.v.trim() || !draft.items.length}>
                  {busy ? "Saving…" : isNew ? "Publish release" : "Save changes"}
                </button>
                <button
                  type="button"
                  className="btn ghost"
                  onClick={() => {
                    const original = entries?.find((e) => e.v === selected);
                    if (original) pick(original);
                    else {
                      setDraft(null);
                      setIsNew(false);
                    }
                  }}
                  disabled={busy}
                >
                  Discard
                </button>
              </div>
            </>
          )}
        </div>
      </div>
    </section>
  );
}
