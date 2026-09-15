"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, useState, type FormEvent } from "react";
import { account, Account, ApiError } from "../lib/account";
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
        <button className="btn ghost" type="submit" aria-disabled={busy}>
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
        <button className="btn ghost" type="submit" aria-disabled={busy}>
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
            <button className="btn danger" onClick={deleteAccount} aria-disabled={busy}>
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
        <button className="btn ghost" type="submit" aria-disabled={busy}>
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
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function logout() {
    setBusy(true);
    try {
      await account.logout();
      router.push("/");
    } catch {
      router.push("/");
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

  return (
    <div className="dash-card">
      <div className="dash-card__head">
        <h2>Sessions</h2>
      </div>
      <p className="lede">Signed in on this device. Sessions last 30 days.</p>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <div className="dash-actions">
        <button className="btn ghost" onClick={logout} aria-disabled={busy}>
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
  const router = useRouter();

  useEffect(() => {
    account
      .me()
      .then(({ user }) => setUser(user))
      .catch(() => router.replace("/login"))
      .finally(() => setChecked(true));
  }, [router]);

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
