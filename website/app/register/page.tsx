"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState, type FormEvent } from "react";
import Logo from "../components/Logo";
import { account, ApiError } from "../lib/account";

export default function RegisterPage() {
  const router = useRouter();
  const [displayName, setDisplayName] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      await account.register(email, password, displayName);
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
          <h1>Create an account</h1>
          <p className="auth-lead">Free — takes a few seconds.</p>

          <form className="stack" onSubmit={onSubmit} noValidate>
            <div className="field">
              <label htmlFor="name">Display name</label>
              <input
                id="name"
                type="text"
                autoComplete="nickname"
                required
                maxLength={60}
                value={displayName}
                onChange={(e) => setDisplayName(e.target.value)}
              />
            </div>
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
              {busy ? "Creating…" : "Create account"}
            </button>
          </form>

          <p className="auth-alt">
            <span>Already have an account?</span>
            <Link href="/login">Sign in</Link>
          </p>
        </div>
      </div>
    </main>
  );
}
