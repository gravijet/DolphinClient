"use client";

import { useEffect, useState } from "react";

interface Platform {
  available: boolean;
  label: string;
  ext: string;
  file?: string;
  url?: string;
  size?: number;
  sha256?: string;
  target?: string;
  arch?: string;
  variants?: Platform[];
}
interface Manifest {
  version: string;
  minecraft?: string;
  platforms: Record<string, Platform>;
}

const ORDER = ["windows", "macos", "linux"] as const;
type Os = (typeof ORDER)[number];

const GLYPHS: Record<string, JSX.Element> = {
  windows: (
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="M3 5.4 11 4.3v7.2H3Zm9-1.2L21 3v8.5h-9ZM3 12.5h8v7.2L3 18.6Zm9 0h9V21l-9-1.2Z" />
    </svg>
  ),
  macos: (
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="M16.4 12.7c0-2.3 1.9-3.4 2-3.5-1.1-1.6-2.8-1.8-3.4-1.9-1.4-.15-2.8.85-3.5.85-.7 0-1.9-.83-3.1-.8-1.6.02-3 .93-3.8 2.36-1.6 2.8-.4 7 1.2 9.3.8 1.1 1.7 2.4 2.9 2.35 1.2-.05 1.6-.75 3-.75 1.4 0 1.8.75 3 .73 1.24-.02 2.02-1.14 2.78-2.25.87-1.27 1.23-2.5 1.25-2.56-.03-.01-2.4-.92-2.98-3.13ZM14.1 5.8c.65-.79 1.09-1.88.97-2.97-.94.04-2.08.63-2.75 1.41-.6.7-1.13 1.82-.99 2.89 1.05.08 2.12-.53 2.77-1.33Z" />
    </svg>
  ),
  linux: (
    <svg viewBox="0 0 24 24" fill="currentColor" fillRule="evenodd" aria-hidden="true">
      <path d="M12 2.2c-2 0-3.3 1.6-3.3 3.9 0 1-.02 1.7-.6 2.5C6.7 10.6 5.1 12.8 5.1 15c0 .9.5 1.5 1.35 1.35.55 1.75 2.85 3.05 5.55 3.05s5-1.3 5.55-3.05C18.4 16.5 18.9 15.9 18.9 15c0-2.2-1.6-4.4-3-6.4-.58-.8-.6-1.5-.6-2.5 0-2.3-1.3-3.9-3.3-3.9Zm-1.5 3.2c.5 0 .85.5.85 1.1s-.35 1.1-.85 1.1-.85-.5-.85-1.1.35-1.1.85-1.1Zm3 0c.5 0 .85.5.85 1.1s-.35 1.1-.85 1.1-.85-.5-.85-1.1.35-1.1.85-1.1Zm-1.5 2.9c.9 0 1.7.5 1.7 1 0 .3-.9.8-1.7.8s-1.7-.5-1.7-.8c0-.5.8-1 1.7-1Z" />
    </svg>
  ),
};

function fmtSize(bytes?: number): string {
  if (!bytes) return "";
  return (bytes / 1024 / 1024).toFixed(1) + " MB";
}

function detectOs(): Os | null {
  if (typeof navigator === "undefined") return null;
  const p = (navigator.platform + " " + navigator.userAgent).toLowerCase();
  if (p.includes("win")) return "windows";
  if (p.includes("mac")) return "macos";
  if (p.includes("linux") || p.includes("x11")) return "linux";
  return null;
}

export default function DownloadCards() {
  const [manifest, setManifest] = useState<Manifest | null>(null);
  const [error, setError] = useState(false);
  const [userOs, setUserOs] = useState<Os | null>(null);

  useEffect(() => {
    setUserOs(detectOs());
    fetch("/downloads/manifest.json", { cache: "no-store" })
      .then((r) => (r.ok ? r.json() : Promise.reject()))
      .then(setManifest)
      .catch(() => setError(true));
  }, []);

  const fallback: Manifest = {
    version: "",
    platforms: {
      windows: { available: false, label: "Windows", ext: "exe" },
      macos: { available: false, label: "macOS", ext: "bin" },
      linux: { available: false, label: "Linux", ext: "bin" },
    },
  };
  const data = manifest ?? fallback;

  return (
    <>
      <div className="dl-grid">
        {ORDER.map((os) => {
          const p = data.platforms[os] ?? fallback.platforms[os];
          const ready = p.available && p.url;
          const variants = (p.variants ?? []).filter((v) => v.available && v.url);
          const isUser = userOs === os;
          return (
            <div
              key={os}
              className={`dl-card${ready ? "" : " soon"}${isUser ? " featured" : ""}`}
            >
              <div className="dl-card__top">
                <span className="dl-os">{GLYPHS[os]}</span>
                {isUser && <span className="dl-badge">Your system</span>}
              </div>
              <h3>{p.label}</h3>
              <div className="dl-ext">
                .{p.ext}
                {ready ? ` · v${data.version} · ${fmtSize(p.size)}` : " · coming soon"}
              </div>
              <div className="dl-meta">
                {ready ? "Installer + auto-updates" : "Coming soon"}
              </div>
              {variants.length > 1 ? (
                <div style={{ display: "grid", gap: ".55rem" }}>
                  {variants.map((variant) => (
                    <a className="btn" href={variant.url} download key={variant.target ?? variant.url}>
                      Download · {variant.arch}
                    </a>
                  ))}
                </div>
              ) : ready ? (
                <a className="btn" href={p.url} download>
                  Download{variants[0]?.arch ? ` · ${variants[0].arch}` : ""}
                </a>
              ) : (
                <span className="btn" aria-disabled="true" role="link">
                  Coming soon
                </span>
              )}
              {ready && p.sha256 && (
                <div className="dl-hash" title={`SHA-256: ${p.sha256}`}>
                  sha256 {p.sha256.slice(0, 16)}…
                </div>
              )}
            </div>
          );
        })}
      </div>
      {error && (
        <p className="status err">
          Couldn't reach the download manifest — please try again later.
        </p>
      )}
    </>
  );
}
