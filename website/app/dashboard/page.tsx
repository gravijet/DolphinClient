"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, useState, type FormEvent } from "react";
import { account, Account, ApiError, Session } from "../lib/account";
import { CHANGELOG_URL, type ChangeEntry } from "../changelog/data";

interface Platform {
  available: boolean;
  size?: number;
}
interface Manifest {
  version: string;
  platforms: Record<string, Platform>;
}

function fmtDate(iso?: string | null): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric" });
}

function fmtDateTime(iso?: string | null): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toLocaleString("en-GB", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
}

/** A rough, good-enough device label — not a real UA parser, just enough
 * to tell one session apart from another at a glance. */
function deviceLabel(ua?: string | null): string {
  if (!ua) return "Unknown device";
  const browser = /Edg\//.test(ua)
    ? "Edge"
    : /Chrome\//.test(ua)
      ? "Chrome"
      : /Firefox\//.test(ua)
        ? "Firefox"
        : /Safari\//.test(ua)
          ? "Safari"
          : "a browser";
  const os = /Windows/.test(ua)
    ? "Windows"
    : /Mac OS X/.test(ua)
      ? "macOS"
      : /Android/.test(ua)
        ? "Android"
        : /iPhone|iPad/.test(ua)
          ? "iOS"
          : /Linux/.test(ua)
            ? "Linux"
            : "an unknown OS";
  return `${browser} on ${os}`;
}

function VerifyStatus({ user }: { user: Account }) {
  const [state, setState] = useState<"idle" | "sending" | "sent" | "error">("idle");
  const [message, setMessage] = useState<string | null>(null);

  async function resend() {
    setState("sending");
    setMessage(null);
    try {
      const r = await account.resendVerification();
      setMessage(r.already_verified ? "Already verified — reload the page." : "Verification email sent — check your inbox.");
      setState("sent");
    } catch (err) {
      setMessage(err instanceof ApiError ? err.message : "couldn't send that, try again");
      setState("error");
    }
  }

  return (
    <>
      <div className="dash-row">
        <span className="k">Status</span>
        <span className={`v ${user.email_verified ? "good" : ""}`}>
          {user.email_verified ? (
            "Verified"
          ) : (
            <>
              Not verified —{" "}
              <button className="linkish" onClick={resend} disabled={state === "sending"}>
                {state === "sending" ? "sending…" : "resend email"}
              </button>
            </>
          )}
        </span>
      </div>
      {message && (
        <p className={state === "error" ? "form-error" : "form-notice"} role={state === "error" ? "alert" : undefined}>
          {message}
        </p>
      )}
    </>
  );
}

function AccountCard({ user, onUpdated }: { user: Account; onUpdated: (u: Account) => void }) {
  const [name, setName] = useState(user.display_name);
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const router = useRouter();

  async function saveName(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setNotice(null);
    setBusy(true);
    try {
      const { user: fresh } = await account.updateProfile({ display_name: name });
      onUpdated(fresh);
      setNotice("Display name updated.");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "update failed");
    } finally {
      setBusy(false);
    }
  }

  async function changePassword(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setNotice(null);
    setBusy(true);
    try {
      await account.changePassword(current, next);
      setCurrent("");
      setNext("");
      setNotice("Password changed.");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "update failed");
    } finally {
      setBusy(false);
    }
  }

  async function deleteAccount() {
    setBusy(true);
    try {
      await account.deleteAccount();
      router.push("/");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "delete failed");
      setBusy(false);
    }
  }

  return (
    <div className="dash-card">
      <div className="dash-card__head">
        <h2>Account</h2>
      </div>
      <div className="dash-row">
        <span className="k">Email</span>
        <span className="v">{user.email}</span>
      </div>
      <VerifyStatus user={user} />
      <div className="dash-row">
        <span className="k">Member since</span>
        <span className="v">{fmtDate(user.created_at)}</span>
      </div>
      <div className="dash-row">
        <span className="k">Last login</span>
        <span className="v">{fmtDate(user.last_login_at)}</span>
      </div>

      <form className="stack" onSubmit={saveName} style={{ marginTop: 18 }}>
        <div className="field">
          <label htmlFor="display_name">Display name</label>
          <input id="display_name" value={name} maxLength={60} onChange={(e) => setName(e.target.value)} />
        </div>
        <button className="btn ghost" type="submit" disabled={busy}>
          Save name
        </button>
      </form>

      <form className="stack" onSubmit={changePassword} style={{ marginTop: 18 }}>
        <div className="field">
          <label htmlFor="current_password">Current password</label>
          <input
            id="current_password"
            type="password"
            autoComplete="current-password"
            value={current}
            onChange={(e) => setCurrent(e.target.value)}
          />
        </div>
        <div className="field">
          <label htmlFor="new_password">New password</label>
          <input
            id="new_password"
            type="password"
            autoComplete="new-password"
            minLength={8}
            value={next}
            onChange={(e) => setNext(e.target.value)}
          />
        </div>
        <button className="btn ghost" type="submit" disabled={busy}>
          Change password
        </button>
      </form>

      {error && (
        <p className="form-error" role="alert" style={{ marginTop: 14 }}>
          {error}
        </p>
      )}
      {notice && (
        <p className="form-notice" style={{ marginTop: 14 }}>
          {notice}
        </p>
      )}

      <div className="dash-actions" style={{ marginTop: 22 }}>
        {confirmDelete ? (
          <>
            <button className="btn danger" onClick={deleteAccount} disabled={busy}>
              Confirm delete
            </button>
            <button className="btn ghost" onClick={() => setConfirmDelete(false)}>
              Cancel
            </button>
          </>
        ) : (
          <button className="linkish" onClick={() => setConfirmDelete(true)}>
            Delete account
          </button>
        )}
      </div>
    </div>
  );
}

function MinecraftCard({ user, onUpdated }: { user: Account; onUpdated: (u: Account) => void }) {
  const [username, setUsername] = useState(user.minecraft_username || "");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function save(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      const { user: fresh } = await account.updateProfile({ minecraft_username: username || null });
      onUpdated(fresh);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "couldn't verify that username");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="dash-card">
      <div className="dash-card__head">
        <h2>Minecraft profile</h2>
      </div>
      <p className="lede">Shown for cosmetics and profile features that need a Java identity.</p>

      {user.minecraft_username && user.minecraft_uuid ? (
        <div className="mc-preview">
          <img
            src={`https://crafatar.com/avatars/${user.minecraft_uuid}?size=48&overlay`}
            alt=""
            width={48}
            height={48}
          />
          <div>
            <div className="mc-preview__name">{user.minecraft_username}</div>
            <div className="mc-preview__uuid">{user.minecraft_uuid}</div>
          </div>
        </div>
      ) : (
        <div className="mc-preview">
          <span className="mc-preview__empty">No Minecraft account linked yet.</span>
        </div>
      )}

      <form className="stack" onSubmit={save}>
        <div className="field">
          <label htmlFor="mc_username">Minecraft username</label>
          <input
            id="mc_username"
            value={username}
            placeholder="e.g. Notch"
            onChange={(e) => setUsername(e.target.value)}
          />
        </div>
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        <button className="btn ghost" type="submit" disabled={busy}>
          {busy ? "Checking…" : "Save"}
        </button>
      </form>
    </div>
  );
}

function DownloadsCard() {
  const [data, setData] = useState<Manifest | null>(null);

  useEffect(() => {
    fetch("/downloads/manifest.json", { cache: "no-store" })
      .then((r) => (r.ok ? r.json() : Promise.reject()))
      .then(setData)
      .catch(() => {});
  }, []);

  return (
    <div className="dash-card">
      <div className="dash-card__head">
        <h2>Downloads</h2>
      </div>
      <p className="lede">{data ? `Current version v${data.version}` : "Loading current version…"}</p>
      {(["windows", "macos", "linux"] as const).map((os) => (
        <div className="dash-row" key={os}>
          <span className="k">{os}</span>
          <span className={`v ${data?.platforms?.[os]?.available ? "good" : ""}`}>
            {data?.platforms?.[os]?.available ? "Available" : "Coming soon"}
          </span>
        </div>
      ))}
      <div className="dash-actions">
        <Link href="/download" className="btn ghost">
          Go to downloads
        </Link>
      </div>
    </div>
  );
}

function ChangelogCard() {
  const [entries, setEntries] = useState<ChangeEntry[]>([]);

  useEffect(() => {
    fetch(CHANGELOG_URL, { cache: "no-store" })
      .then((r) => (r.ok ? r.json() : Promise.reject()))
      .then((body) => setEntries((body.entries ?? body).slice(0, 3)))
      .catch(() => {});
  }, []);

  return (
    <div className="dash-card">
      <div className="dash-card__head">
        <h2>Recent changes</h2>
      </div>
      {entries.length === 0 && <p className="lede">Loading…</p>}
      {entries.map((e) => (
        <div className="dash-row" key={e.v}>
          <span className="k">{e.v}</span>
          <span className="v">{e.date.replace(/^Current · /, "")}</span>
        </div>
      ))}
      <div className="dash-actions">
        <Link href="/changelog" className="btn ghost">
          Full changelog
        </Link>
      </div>
    </div>
  );
}

function SessionsCard() {
  const router = useRouter();
  const [sessions, setSessions] = useState<Session[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [revoking, setRevoking] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  function load() {
    account
      .listSessions()
      .then(({ sessions }) => setSessions(sessions))
      .catch(() => setSessions([]));
  }

  useEffect(load, []);

  async function logout() {
    setBusy(true);
    setError(null);
    try {
      await account.logout();
      router.push("/");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "couldn't sign out, try again");
      setBusy(false);
    }
  }

  async function logoutAll() {
    setBusy(true);
    setError(null);
    try {
      await account.logoutAll();
      router.push("/login");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "failed");
      setBusy(false);
    }
  }

  async function revoke(id: string) {
    setRevoking(id);
    setError(null);
    try {
      await account.revokeSession(id);
      setSessions((prev) => (prev ? prev.filter((s) => s.id !== id) : prev));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "couldn't revoke that session");
    } finally {
      setRevoking(null);
    }
  }

  return (
    <div className="dash-card">
      <div className="dash-card__head">
        <h2>Sessions</h2>
      </div>
      <p className="lede">Everywhere you're signed in. Sessions last 30 days.</p>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      {sessions === null && <p className="dash__loading">Loading sessions…</p>}
      {sessions?.map((s) => (
        <div className="dash-row" key={s.id}>
          <span className="k">
            {deviceLabel(s.user_agent)}
            {s.current && " (this device)"}
          </span>
          <span className="v">
            {s.ip ? `${s.ip} · ` : ""}
            {fmtDateTime(s.created_at)}
            {!s.current && (
              <>
                {" · "}
                <button className="linkish" onClick={() => revoke(s.id)} disabled={revoking === s.id}>
                  {revoking === s.id ? "revoking…" : "revoke"}
                </button>
              </>
            )}
          </span>
        </div>
      ))}
      <div className="dash-actions" style={{ marginTop: 18 }}>
        <button className="btn ghost" onClick={logout} disabled={busy}>
          Sign out
        </button>
        <button className="linkish" onClick={logoutAll}>
          Sign out everywhere
        </button>
      </div>
    </div>
  );
}

export default function DashboardPage() {
  const [user, setUser] = useState<Account | null>(null);
  const [checked, setChecked] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const router = useRouter();

  useEffect(() => {
    account
      .me()
      .then(({ user }) => setUser(user))
      .catch((err) => {
        // Only a real "not signed in" bounces to /login — a network hiccup
        // or a brief account-api outage should say so, not sign the user out.
        if (err instanceof ApiError && err.status === 401) {
          router.replace("/login");
        } else {
          setLoadError("Couldn't reach the account service. Check your connection and reload.");
        }
      })
      .finally(() => setChecked(true));
  }, [router]);

  if (checked && loadError) {
    return (
      <main className="dash wide">
        <p className="form-error" role="alert">
          {loadError}
        </p>
      </main>
    );
  }

  if (!checked || !user) {
    return (
      <main className="dash wide">
        <p className="dash__loading">Loading your account…</p>
      </main>
    );
  }

  return (
    <main className="dash wide">
      <div className="dash__head">
        <div>
          <h1>Welcome, {user.display_name}.</h1>
          <p>Your DolphinClient account.</p>
        </div>
      </div>

      <div className="dash__grid">
        <AccountCard user={user} onUpdated={setUser} />
        <MinecraftCard user={user} onUpdated={setUser} />
        <DownloadsCard />
        <ChangelogCard />
      </div>
      <div className="dash__grid wide" style={{ marginTop: 18 }}>
        <SessionsCard />
      </div>
    </main>
  );
}
