"use client";

import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { Suspense, useEffect, useState } from "react";
import Logo from "../components/Logo";
import { account, ApiError } from "../lib/account";

type Status = "checking" | "done" | "error";

function VerifyBody() {
  const token = useSearchParams().get("token") || "";
  const [status, setStatus] = useState<Status>(token ? "checking" : "error");
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    if (!token) return;
    account
      .verifyEmail(token)
      .then(() => setStatus("done"))
      .catch((err) => {
        setMessage(err instanceof ApiError ? err.message : "couldn't reach the account service");
        setStatus("error");
      });
  }, [token]);

  if (!token) {
    return (
      <p className="form-error" role="alert">
        This link is missing its token — open the link from your email again.
      </p>
    );
  }
  if (status === "checking") {
    return <p className="dash__loading">Confirming your email…</p>;
  }
  if (status === "error") {
    return (
      <p className="form-error" role="alert">
        {message}
      </p>
    );
  }
  return <p className="form-notice">Your email is verified.</p>;
}

export default function VerifyPage() {
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
          <h1>Email verification</h1>

          <Suspense fallback={null}>
            <VerifyBody />
          </Suspense>

          <p className="auth-alt">
            <Link href="/dashboard">Back to dashboard</Link>
          </p>
        </div>
      </div>
    </main>
  );
}
