"use client";

import Link from "next/link";
import { useState, type FormEvent } from "react";
import Logo from "../components/Logo";
import { account, ApiError } from "../lib/account";

export default function ForgotPage() {
  const [email, setEmail] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [sent, setSent] = useState(false);
  const [busy, setBusy] = useState(false);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      await account.forgot(email);
      setSent(true);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "couldn't reach the account service");
    } finally {
      setBusy(false);
    }
  }

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
          <h1>Reset your password</h1>
          <p className="auth-lead">
            We&rsquo;ll email a reset link if that address has an account.
          </p>

          {sent ? (
            <p className="form-notice" style={{ marginTop: 24 }}>
              If that email exists, a reset link is on its way — check your inbox.
            </p>
          ) : (
            <form className="stack" onSubmit={onSubmit} noValidate>
              <div className="field">
                <label htmlFor="email">Email</label>
                <input
                  id="email"
                  type="email"
                  autoComplete="username"
                  required
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                />
              </div>
              {error && (
                <p className="form-error" role="alert">
                  {error}
                </p>
              )}
              <button className="btn block" type="submit" disabled={busy}>
                {busy ? "Sending…" : "Send reset link"}
              </button>
            </form>
          )}

          <p className="auth-alt">
            <Link href="/login">Back to sign in</Link>
          </p>
        </div>
      </div>
    </main>
  );
}
