"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState, type FormEvent } from "react";
import Logo from "../components/Logo";
import { account, ApiError } from "../lib/account";

export default function LoginPage() {
  const router = useRouter();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // Set once /auth/login answers with {requires_totp: true} — from then on
  // the form asks for a 6-digit code (or a backup code) instead of the
  // password, and submits against /auth/login/totp with this challenge.
  const [challenge, setChallenge] = useState<string | null>(null);
  const [code, setCode] = useState("");

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      const result = await account.login(email, password);
      if ("requires_totp" in result) {
        setChallenge(result.challenge);
      } else {
        router.push("/dashboard");
      }
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "couldn't reach the account service");
    } finally {
      setBusy(false);
    }
  }

  async function onSubmitTotp(e: FormEvent) {
    e.preventDefault();
    if (!challenge) return;
    setError(null);
    setBusy(true);
    try {
      await account.loginTotp(challenge, code);
      router.push("/dashboard");
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
          {challenge ? (
            <>
              <h1>Two-factor code</h1>
              <p className="auth-lead">
                Enter the 6-digit code from your authenticator app, or one of your backup codes.
              </p>
              <form className="stack" onSubmit={onSubmitTotp} noValidate>
                <div className="field">
                  <label htmlFor="totp_code">Code</label>
                  <input
                    id="totp_code"
                    type="text"
                    inputMode="text"
                    autoComplete="one-time-code"
                    autoFocus
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
                <button className="btn block" type="submit" disabled={busy}>
                  {busy ? "Verifying…" : "Verify"}
                </button>
                <button
                  type="button"
                  className="linkish"
                  onClick={() => {
                    setChallenge(null);
                    setCode("");
                    setError(null);
                  }}
                >
                  Back to sign in
                </button>
              </form>
            </>
          ) : (
            <>
              <h1>Sign in</h1>
              <p className="auth-lead">Access your DolphinClient account.</p>

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
                <div className="field">
                  <label htmlFor="password">Password</label>
                  <input
                    id="password"
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
                <button className="btn block" type="submit" disabled={busy}>
                  {busy ? "Signing in…" : "Sign in"}
                </button>
              </form>

              <p className="auth-alt">
                <Link href="/forgot">Forgot password?</Link>
                <span aria-hidden="true">·</span>
                <Link href="/register">Create an account</Link>
              </p>
            </>
          )}
        </div>
      </div>
    </main>
  );
}
