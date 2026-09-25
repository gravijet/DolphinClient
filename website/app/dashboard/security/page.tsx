"use client";

import { useRouter } from "next/navigation";
import { useEffect, useState, type FormEvent } from "react";
import QRCode from "qrcode";
import { account, Account, ApiError, LoginHistoryEntry } from "../../lib/account";
import DashNav from "../DashNav";

function fmtDateTime(iso?: string | null): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toLocaleString("en-GB", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
}

function BackupCodes({ codes }: { codes: string[] }) {
  const [copied, setCopied] = useState(false);

  function copyAll() {
    navigator.clipboard?.writeText(codes.join("\n")).then(
      () => setCopied(true),
      () => {},
    );
  }

  function download() {
    const blob = new Blob([codes.join("\n") + "\n"], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "dolphinclient-backup-codes.txt";
    a.click();
    URL.revokeObjectURL(url);
  }

  return (
    <div>
      <p className="form-notice">
        Save these — each works once, and this is the only time they're shown.
      </p>
      <div className="backup-codes">
        {codes.map((c) => (
          <code key={c}>{c}</code>
        ))}
      </div>
      <div className="dash-actions">
        <button type="button" className="btn ghost" onClick={copyAll}>
          {copied ? "Copied" : "Copy all"}
        </button>
        <button type="button" className="btn ghost" onClick={download}>
          Download .txt
        </button>
      </div>
    </div>
  );
}

function TotpSetup({ onDone }: { onDone: () => void }) {
  const [secret, setSecret] = useState<string | null>(null);
  const [qr, setQr] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [backupCodes, setBackupCodes] = useState<string[] | null>(null);

  async function start() {
    setError(null);
    setBusy(true);
    try {
      const r = await account.totpSetup();
      setSecret(r.secret);
      const dataUrl = await QRCode.toDataURL(r.otpauth_url, { margin: 1, width: 200 });
      setQr(dataUrl);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "couldn't start setup");
    } finally {
      setBusy(false);
    }
  }

  async function confirm(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      const r = await account.totpConfirm(code);
      setBackupCodes(r.backup_codes);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "couldn't confirm that code");
    } finally {
      setBusy(false);
    }
  }

  if (backupCodes) {
    return (
      <div>
        <p style={{ marginBottom: 8 }}>
          <strong>Two-factor authentication is now on.</strong>
        </p>
        <BackupCodes codes={backupCodes} />
        <div className="dash-actions">
          <button type="button" className="btn" onClick={onDone}>
            Done
          </button>
        </div>
      </div>
    );
  }

  if (!secret) {
    return (
      <div>
        <p className="lede">
          Add a second step to sign-in: a 6-digit code from an authenticator app (Google
          Authenticator, Authy, 1Password, …), in addition to your password.
        </p>
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        <button type="button" className="btn" onClick={start} disabled={busy}>
          {busy ? "Starting…" : "Set up two-factor authentication"}
        </button>
      </div>
    );
  }

  return (
    <div>
      <p className="lede">Scan this with your authenticator app, then enter the 6-digit code it shows.</p>
      {qr && (
        <div className="totp-qr">
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img src={qr} alt="TOTP setup QR code" />
        </div>
      )}
      <span className="totp-secret">{secret}</span>
      <form className="stack" onSubmit={confirm}>
        <div className="field">
          <label htmlFor="setup_code">6-digit code</label>
          <input
            id="setup_code"
            type="text"
            autoComplete="one-time-code"
            required
            value={code}
            onChange={(e) => setCode(e.target.value)}
          />
        </div>
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        <button className="btn" type="submit" disabled={busy}>
          {busy ? "Confirming…" : "Confirm and enable"}
        </button>
      </form>
    </div>
  );
}

function TotpDisable({ onDone }: { onDone: () => void }) {
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function disable(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      await account.totpDisable(password, code);
      onDone();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "couldn't disable two-factor authentication");
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="stack" onSubmit={disable}>
      <div className="field">
        <label htmlFor="disable_password">Password</label>
        <input
          id="disable_password"
          type="password"
          autoComplete="current-password"
          required
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
      </div>
      <div className="field">
        <label htmlFor="disable_code">Code (app or backup)</label>
        <input
          id="disable_code"
          type="text"
          required
          value={code}
          onChange={(e) => setCode(e.target.value)}
        />
      </div>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <button className="btn danger" type="submit" disabled={busy}>
        {busy ? "Disabling…" : "Disable two-factor authentication"}
      </button>
    </form>
  );
}

function RegenerateBackupCodes() {
  const [open, setOpen] = useState(false);
  const [password, setPassword] = useState("");
  const [codes, setCodes] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function regenerate(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      const r = await account.totpRegenerateBackupCodes(password);
      setCodes(r.backup_codes);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "couldn't regenerate codes");
    } finally {
      setBusy(false);
    }
  }

  if (codes) return <BackupCodes codes={codes} />;

  if (!open) {
    return (
      <button type="button" className="linkish" onClick={() => setOpen(true)}>
        Regenerate backup codes
      </button>
    );
  }

  return (
    <form className="stack" onSubmit={regenerate}>
      <div className="field">
        <label htmlFor="regen_password">Password</label>
        <input
          id="regen_password"
          type="password"
          autoComplete="current-password"
          required
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
      </div>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <button className="btn ghost" type="submit" disabled={busy}>
        {busy ? "Regenerating…" : "Regenerate — invalidates the old codes"}
      </button>
    </form>
  );
}

function TotpCard({ user, onUpdated }: { user: Account; onUpdated: () => void }) {
  const [mode, setMode] = useState<"idle" | "setup" | "disable">("idle");

  return (
    <div className="dash-card">
      <div className="dash-card__head">
        <h2>Two-factor authentication</h2>
      </div>
      <div className="totp-status" style={{ marginBottom: 16 }}>
        <span className={`totp-badge ${user.totp_enabled ? "on" : "off"}`}>
          {user.totp_enabled ? "Enabled" : "Disabled"}
        </span>
      </div>

      {mode === "idle" && user.totp_enabled && (
        <div className="dash-actions" style={{ flexDirection: "column", alignItems: "flex-start", gap: 12 }}>
          <RegenerateBackupCodes />
          <button type="button" className="linkish" onClick={() => setMode("disable")}>
            Disable two-factor authentication
          </button>
        </div>
      )}

      {mode === "idle" && !user.totp_enabled && (
        <button type="button" className="btn" onClick={() => setMode("setup")}>
          Set up two-factor authentication
        </button>
      )}

      {mode === "setup" && (
        <TotpSetup
          onDone={() => {
            setMode("idle");
            onUpdated();
          }}
        />
      )}

      {mode === "disable" && (
        <TotpDisable
          onDone={() => {
            setMode("idle");
            onUpdated();
          }}
        />
      )}
    </div>
  );
}

function LoginHistoryCard() {
  const [history, setHistory] = useState<LoginHistoryEntry[] | null>(null);

  useEffect(() => {
    account
      .loginHistory()
      .then((r) => setHistory(r.history))
      .catch(() => setHistory([]));
  }, []);

  return (
    <div className="dash-card">
      <div className="dash-card__head">
        <h2>Login activity</h2>
      </div>
      <p className="lede">Every sign-in attempt on this account, most recent first.</p>
      {history === null && <p className="dash__loading">Loading…</p>}
      {history?.length === 0 && <p style={{ color: "var(--faint)" }}>Nothing recorded yet.</p>}
      {history && history.length > 0 && (
        <div style={{ overflowX: "auto" }}>
          <table className="login-history">
            <thead>
              <tr>
                <th>When</th>
                <th>Result</th>
                <th>Location</th>
                <th>IP</th>
              </tr>
            </thead>
            <tbody>
              {history.map((h, i) => (
                <tr key={i}>
                  <td>{fmtDateTime(h.created_at)}</td>
                  <td className={h.success ? "ok" : "bad"}>
                    {h.success ? "Success" : h.reason === "banned" ? "Blocked (banned)" : "Failed"}
                  </td>
                  <td>{h.city && h.country ? `${h.city}, ${h.country}` : h.country || "—"}</td>
                  <td>{h.ip || "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

export default function SecurityPage() {
  const router = useRouter();
  const [user, setUser] = useState<Account | null>(null);
  const [checked, setChecked] = useState(false);

  function load() {
    account
      .me()
      .then(({ user }) => setUser(user))
      .catch((err) => {
        if (err instanceof ApiError && err.status === 401) router.replace("/login");
      })
      .finally(() => setChecked(true));
  }

  useEffect(load, [router]);

  if (!checked || !user) {
    return (
      <main className="dash wide">
        <DashNav />
        <p className="dash__loading">Loading your security settings…</p>
      </main>
    );
  }

  return (
    <main className="dash wide">
      <DashNav />
      <div className="dash__head">
        <div>
          <h1>Security</h1>
          <p>Two-factor authentication and where this account has signed in.</p>
        </div>
      </div>

      <div className="dash__grid wide">
        <TotpCard user={user} onUpdated={load} />
        <LoginHistoryCard />
      </div>
    </main>
  );
}
