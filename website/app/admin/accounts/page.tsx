"use client";

import { useState, useEffect } from "react";
import Link from "next/link";
import { admin, AdminAccount, ApiError } from "../../lib/account";

const LIMIT = 50;

function fmtDate(iso?: string | null): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric" });
}

export default function AccountsPage() {
  const [accounts, setAccounts] = useState<AdminAccount[]>([]);
  const [search, setSearch] = useState("");
  const [offset, setOffset] = useState(0);
  const [hasMore, setHasMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<number | null>(null);

  async function load(q: string, off: number) {
    setLoading(true);
    setError(null);
    try {
      const result = await admin.listAccounts(q, LIMIT, off);
      setAccounts(result.accounts);
      setHasMore(result.hasMore);
      setOffset(off);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "failed to load accounts");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    load(search, 0);
  }, [search]);

  async function toggleBan(id: number, currentlyBanned: boolean) {
    setBusy(id);
    try {
      if (currentlyBanned) {
        await admin.unbanAccount(id);
      } else {
        await admin.banAccount(id);
      }
      setAccounts((prev) =>
        prev.map((a) => (a.id === id ? { ...a, banned: !a.banned } : a)),
      );
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "failed to update account");
    } finally {
      setBusy(null);
    }
  }

  async function deleteAccountConfirm(id: number, email: string) {
    if (!confirm(`Delete account ${email}? This cannot be undone.`)) return;
    setBusy(id);
    try {
      await admin.deleteAccount(id);
      setAccounts((prev) => prev.filter((a) => a.id !== id));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "failed to delete account");
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="admin-page">
      <div style={{ marginBottom: 32 }}>
        <Link href="/admin" className="linkish">
          ← Back
        </Link>
      </div>

      <div style={{ marginBottom: 24 }}>
        <input
          type="text"
          placeholder="Search by email or name…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          style={{
            width: "100%",
            maxWidth: 400,
            padding: "8px 12px",
            border: "1px solid var(--gray-400)",
            borderRadius: 4,
            fontSize: 14,
          }}
        />
      </div>

      {error && (
        <p className="form-error" style={{ marginBottom: 16 }}>
          {error}
        </p>
      )}

      {loading && <p>Loading…</p>}

      {!loading && accounts.length === 0 && <p>No accounts found.</p>}

      {!loading && accounts.length > 0 && (
        <>
          <div style={{ overflowX: "auto", marginBottom: 24 }}>
            <table
              style={{
                width: "100%",
                borderCollapse: "collapse",
                fontSize: 14,
              }}
            >
              <thead>
                <tr style={{ borderBottom: "1px solid var(--gray-300)" }}>
                  <th style={{ padding: "12px", textAlign: "left", fontWeight: 600 }}>Email</th>
                  <th style={{ padding: "12px", textAlign: "left", fontWeight: 600 }}>Display Name</th>
                  <th style={{ padding: "12px", textAlign: "left", fontWeight: 600 }}>Created</th>
                  <th style={{ padding: "12px", textAlign: "left", fontWeight: 600 }}>Last Login</th>
                  <th style={{ padding: "12px", textAlign: "center", fontWeight: 600 }}>Status</th>
                  <th style={{ padding: "12px", textAlign: "center", fontWeight: 600 }}>Actions</th>
                </tr>
              </thead>
              <tbody>
                {accounts.map((acc) => (
                  <tr
                    key={acc.id}
                    style={{
                      borderBottom: "1px solid var(--gray-300)",
                      opacity: acc.banned ? 0.6 : 1,
                    }}
                  >
                    <td style={{ padding: "12px" }}>{acc.email}</td>
                    <td style={{ padding: "12px" }}>{acc.display_name}</td>
                    <td style={{ padding: "12px" }}>{fmtDate(acc.created_at)}</td>
                    <td style={{ padding: "12px" }}>{fmtDate(acc.last_login_at)}</td>
                    <td
                      style={{
                        padding: "12px",
                        textAlign: "center",
                        color: acc.banned ? "var(--red-600)" : "var(--green-600)",
                      }}
                    >
                      {acc.banned ? "Banned" : "Active"}
                    </td>
                    <td
                      style={{
                        padding: "12px",
                        textAlign: "center",
                        display: "flex",
                        gap: 8,
                        justifyContent: "center",
                      }}
                    >
                      <button
                        className="btn ghost"
                        style={{ fontSize: 12, padding: "4px 8px" }}
                        onClick={() => toggleBan(acc.id, acc.banned)}
                        disabled={busy === acc.id}
                      >
                        {busy === acc.id ? "…" : acc.banned ? "Unban" : "Ban"}
                      </button>
                      <button
                        className="btn danger"
                        style={{ fontSize: 12, padding: "4px 8px" }}
                        onClick={() => deleteAccountConfirm(acc.id, acc.email)}
                        disabled={busy === acc.id}
                      >
                        {busy === acc.id ? "…" : "Delete"}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <div style={{ display: "flex", gap: 12, justifyContent: "center" }}>
            <button
              className="btn ghost"
              onClick={() => load(search, Math.max(0, offset - LIMIT))}
              disabled={offset === 0 || loading}
            >
              Previous
            </button>
            <span>
              Showing {offset + 1}–{offset + accounts.length}
              {hasMore && ` (more available)`}
            </span>
            <button
              className="btn ghost"
              onClick={() => load(search, offset + LIMIT)}
              disabled={!hasMore || loading}
            >
              Next
            </button>
          </div>
        </>
      )}
    </div>
  );
}
