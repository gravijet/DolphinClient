"use client";

import { useRouter } from "next/navigation";
import { useEffect, useState, type FormEvent } from "react";
import { account, Account, ApiError, SocialLinks } from "../../lib/account";
import DashNav from "../DashNav";

const SOCIAL_FIELDS: { key: keyof SocialLinks; label: string }[] = [
  { key: "twitter", label: "Twitter / X" },
  { key: "discord", label: "Discord" },
  { key: "github", label: "GitHub" },
  { key: "youtube", label: "YouTube" },
];

export default function ProfilePage() {
  const router = useRouter();
  const [user, setUser] = useState<Account | null>(null);
  const [checked, setChecked] = useState(false);

  const [bio, setBio] = useState("");
  const [avatarUrl, setAvatarUrl] = useState("");
  const [social, setSocial] = useState<SocialLinks>({});
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    account
      .me()
      .then(({ user }) => {
        setUser(user);
        setBio(user.bio);
        setAvatarUrl(user.avatar_url);
        setSocial(user.social_links);
      })
      .catch((err) => {
        if (err instanceof ApiError && err.status === 401) router.replace("/login");
      })
      .finally(() => setChecked(true));
  }, [router]);

  async function save(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setNotice(null);
    setBusy(true);
    try {
      const { user: fresh } = await account.updateProfile({
        bio,
        avatar_url: avatarUrl,
        social_links: social,
      });
      setUser(fresh);
      setNotice("Profile updated.");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "update failed");
    } finally {
      setBusy(false);
    }
  }

  if (!checked || !user) {
    return (
      <main className="dash wide">
        <DashNav />
        <p className="dash__loading">Loading your profile…</p>
      </main>
    );
  }

  return (
    <main className="dash wide">
      <DashNav />
      <div className="dash__head">
        <div>
          <h1>Profile</h1>
          <p>How your account looks to the rest of DolphinClient.</p>
        </div>
      </div>

      <div className="dash__grid wide">
        <div className="dash-card">
          <div className="dash-card__head">
            <h2>Public profile</h2>
          </div>

          <div className="avatar-preview">
            {avatarUrl ? (
              // eslint-disable-next-line @next/next/no-img-element
              <img src={avatarUrl} alt="" onError={(e) => (e.currentTarget.style.visibility = "hidden")} />
            ) : (
              <div className="avatar-preview__empty">No avatar</div>
            )}
            <div>
              <div style={{ fontWeight: 600 }}>{user.display_name}</div>
              <div style={{ color: "var(--faint)", fontSize: "0.82rem" }}>{user.email}</div>
            </div>
          </div>

          <form className="stack" onSubmit={save}>
            <div className="field">
              <label htmlFor="avatar_url">Avatar URL</label>
              <input
                id="avatar_url"
                type="text"
                placeholder="https://…"
                value={avatarUrl}
                onChange={(e) => setAvatarUrl(e.target.value)}
              />
              <span className="hint">A direct link to an image — nothing is uploaded or hosted here.</span>
            </div>

            <div className="field">
              <label htmlFor="bio">Bio</label>
              <textarea
                id="bio"
                rows={4}
                maxLength={500}
                value={bio}
                onChange={(e) => setBio(e.target.value)}
              />
              <span className="hint">{bio.length}/500</span>
            </div>

            <div className="field">
              <label>Social links</label>
              <div className="stack">
                {SOCIAL_FIELDS.map((f) => (
                  <div className="social-row" key={f.key}>
                    <label htmlFor={`social_${f.key}`}>{f.label}</label>
                    <input
                      id={`social_${f.key}`}
                      type="text"
                      placeholder="handle"
                      value={social[f.key] || ""}
                      onChange={(e) => setSocial((s) => ({ ...s, [f.key]: e.target.value }))}
                    />
                  </div>
                ))}
              </div>
            </div>

            {error && (
              <p className="form-error" role="alert">
                {error}
              </p>
            )}
            {notice && <p className="form-notice">{notice}</p>}

            <button className="btn" type="submit" disabled={busy}>
              {busy ? "Saving…" : "Save profile"}
            </button>
          </form>
        </div>
      </div>
    </main>
  );
}
