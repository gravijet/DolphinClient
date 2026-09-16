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

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      await account.login(email, password);
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
        </div>
      </div>
    </main>
  );
}
