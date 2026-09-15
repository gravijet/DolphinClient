"use client";

import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { Suspense, useState, type FormEvent } from "react";
import Logo from "../components/Logo";
import { account, ApiError } from "../lib/account";

function ResetForm() {
  const router = useRouter();
  const token = useSearchParams().get("token") || "";
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      await account.reset(token, password);
      router.push("/login");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "couldn't reach the account service");
    } finally {
      setBusy(false);
    }
  }

  if (!token) {
    return (
      <p className="form-error" style={{ marginTop: 24 }}>
        This link is missing its token — open the link from your email again.
      </p>
    );
  }

  return (
    <form className="stack" onSubmit={onSubmit} noValidate>
      <div className="field">
        <label htmlFor="password">New password</label>
        <input
          id="password"
          type="password"
          autoComplete="new-password"
          required
          minLength={8}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        <span className="hint">At least 8 characters.</span>
      </div>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <button className="btn block" type="submit" aria-disabled={busy}>
        {busy ? "Saving…" : "Set new password"}
      </button>
    </form>
  );
}

export default function ResetPage() {
  return (
    <main className="auth">
      <div className="auth-box">
        <Link href="/" className="brand">
          <Logo />
          <span>
            Dolphin<span className="accent">Client</span>
          </span>
        </Link>

        <div className="auth-card">
          <h1>Set a new password</h1>
          <p className="auth-lead">This link is valid for one hour.</p>

          <Suspense fallback={null}>
            <ResetForm />
          </Suspense>

          <p className="auth-alt">
            <Link href="/login">Back to sign in</Link>
          </p>
        </div>
      </div>
    </main>
  );
}
