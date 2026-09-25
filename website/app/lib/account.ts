"use client";

// The account API talks to account-api.mjs (deploy/account-api.mjs). In
// production it's nginx's job to proxy /account-api/ to that service on the
// same origin — see deploy/nginx/dolphinclient-locations.conf — so the
// default here is a same-origin relative path. NEXT_PUBLIC_ACCOUNT_API only
// exists to point a local `next dev` at a local `node account-api.mjs`
// running on its own port, since those are two different origins.
const API_BASE = process.env.NEXT_PUBLIC_ACCOUNT_API || "/account-api";

export interface Account {
  id: number;
  email: string;
  display_name: string;
  minecraft_username: string | null;
  minecraft_uuid: string | null;
  created_at: string;
  last_login_at: string | null;
  email_verified: boolean;
}

export interface Session {
  id: string;
  created_at: string;
  expires_at: string;
  ip: string | null;
  user_agent: string | null;
  current: boolean;
}

export interface AdminAccount {
  id: number;
  email: string;
  display_name: string;
  minecraft_username: string | null;
  created_at: string;
  last_login_at: string | null;
  banned: boolean;
}

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

async function call<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    credentials: "include",
    headers: init?.body ? { "content-type": "application/json" } : undefined,
    ...init,
  });
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(res.status, body.error || `request failed (${res.status})`);
  return body as T;
}

async function adminCall<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`/admin-api${path}`, {
    credentials: "include",
    headers: init?.body ? { "content-type": "application/json" } : undefined,
    ...init,
  });
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(res.status, body.error || `request failed (${res.status})`);
  return body as T;
}

export const account = {
  me: () => call<{ user: Account }>("/me"),
  register: (email: string, password: string, display_name: string) =>
    call<{ user: Account }>("/auth/register", {
      method: "POST",
      body: JSON.stringify({ email, password, display_name }),
    }),
  login: (email: string, password: string) =>
    call<{ user: Account }>("/auth/login", {
      method: "POST",
      body: JSON.stringify({ email, password }),
    }),
  logout: () => call<{ ok: true }>("/auth/logout", { method: "POST" }),
  logoutAll: () => call<{ ok: true }>("/auth/logout-all", { method: "POST" }),
  forgot: (email: string) =>
    call<{ ok: true; message: string }>("/auth/forgot", {
      method: "POST",
      body: JSON.stringify({ email }),
    }),
  reset: (token: string, password: string) =>
    call<{ ok: true }>("/auth/reset", {
      method: "POST",
      body: JSON.stringify({ token, password }),
    }),
  changePassword: (current_password: string, new_password: string) =>
    call<{ ok: true }>("/auth/change-password", {
      method: "POST",
      body: JSON.stringify({ current_password, new_password }),
    }),
  updateProfile: (patch: { display_name?: string; minecraft_username?: string | null }) =>
    call<{ user: Account }>("/profile", { method: "PATCH", body: JSON.stringify(patch) }),
  deleteAccount: () => call<{ ok: true }>("/me", { method: "DELETE" }),
  verifyEmail: (token: string) =>
    call<{ ok: true }>("/auth/verify", { method: "POST", body: JSON.stringify({ token }) }),
  resendVerification: () =>
    call<{ ok: true; already_verified?: boolean }>("/auth/resend-verification", { method: "POST" }),
  listSessions: () => call<{ sessions: Session[] }>("/sessions"),
  revokeSession: (id: string) => call<{ ok: true }>(`/sessions/${encodeURIComponent(id)}`, { method: "DELETE" }),
};

export const admin = {
  listAccounts: (query?: string, limit?: number, offset?: number) =>
    adminCall<{ accounts: AdminAccount[]; hasMore: boolean; offset: number }>(
      `/accounts?${new URLSearchParams({
        ...(query && { q: query }),
        ...(limit && { limit: String(limit) }),
        ...(offset && { offset: String(offset) }),
      }).toString()}`,
    ),
  banAccount: (id: number) =>
    adminCall<{ id: number; banned: boolean }>(`/accounts/${id}`, {
      method: "PATCH",
      body: JSON.stringify({ banned: true }),
    }),
  unbanAccount: (id: number) =>
    adminCall<{ id: number; banned: boolean }>(`/accounts/${id}`, {
      method: "PATCH",
      body: JSON.stringify({ banned: false }),
    }),
  deleteAccount: (id: number) =>
    adminCall<{ id: number; deleted_email: string }>(`/accounts/${id}`, { method: "DELETE" }),
};
