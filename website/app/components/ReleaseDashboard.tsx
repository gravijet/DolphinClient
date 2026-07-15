"use client";

import { useEffect, useState } from "react";
import Logo from "./Logo";

/**
 * The hero centrepiece: a real release dashboard. Every value here is read live
 * from the published /downloads/manifest.json — the same file the launcher uses
 * to auto-update — so nothing is invented. No performance figures, no mock data:
 * just the honest state of the current build (version, target, platforms, size,
 * checksum, date).
 */

interface Platform {
  available: boolean;
  label: string;
  ext: string;
  size?: number;
  sha256?: string;
}
interface Manifest {
  version: string;
  minecraft?: string;
  generatedAt?: string;
  platforms: Record<string, Platform>;
  clientVersions?: unknown[];
}

function fmtSize(bytes?: number): string {
  if (!bytes) return "—";
  return (bytes / 1024 / 1024).toFixed(1) + " MB";
}

function fmtDate(iso?: string): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric" });
}

export default function ReleaseDashboard() {
  const [data, setData] = useState<Manifest | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    fetch("/downloads/manifest.json", { cache: "no-store" })
      .then((r) => (r.ok ? r.json() : Promise.reject()))
      .then(setData)
      .catch(() => setError(true));
  }, []);

  const win = data?.platforms?.windows;
  const releaseCount = Array.isArray(data?.clientVersions) ? data!.clientVersions!.length : null;

  const platformState = (p?: Platform) =>
    p?.available ? { text: "Available", cls: "good" } : { text: "Coming soon", cls: "soon" };
  const mac = platformState(data?.platforms?.macos);
  const lin = platformState(data?.platforms?.linux);

  return (
    <div className="readout" role="group" aria-label="DolphinClient release status">
      <div className="readout__top">
        <Logo />
        <span>release status</span>
        <span className="readout__dot" />
      </div>

      <div className="readout__fps">
        <span className="big">{data ? `v${data.version}` : error ? "—" : "…"}</span>
        <span className="unit">for Minecraft {data?.minecraft ?? "26.1"}</span>
      </div>

      <div className="readout__rows">
        <div className="readout__row">
          <span className="k">windows</span>
          <span className="l" />
          <span className="v good">{win?.available ? "Available" : "Coming soon"}</span>
        </div>
        <div className="readout__row">
          <span className="k">macos</span>
          <span className="l" />
          <span className={`v ${mac.cls}`}>{mac.text}</span>
        </div>
        <div className="readout__row">
          <span className="k">linux</span>
          <span className="l" />
          <span className={`v ${lin.cls}`}>{lin.text}</span>
        </div>
        <div className="readout__row">
          <span className="k">download</span>
          <span className="l" />
          <span className="v">{fmtSize(win?.size)}</span>
        </div>
        <div className="readout__row">
          <span className="k">verified</span>
          <span className="l" />
          <span className="v good" title={win?.sha256 ? `SHA-256: ${win.sha256}` : undefined}>
            {win?.sha256 ? "SHA-256 ✓" : "—"}
          </span>
        </div>
        <div className="readout__row">
          <span className="k">updated</span>
          <span className="l" />
          <span className="v">{fmtDate(data?.generatedAt)}</span>
        </div>
        {releaseCount !== null && (
          <div className="readout__row">
            <span className="k">releases</span>
            <span className="l" />
            <span className="v">{releaseCount}</span>
          </div>
        )}
      </div>
    </div>
  );
}
