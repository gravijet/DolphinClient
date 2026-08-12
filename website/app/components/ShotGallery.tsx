"use client";

// Screenshots for one release. The thumbnails sit in the changelog entry; a
// click opens a full-screen viewer (Esc closes, ← / → step through it).
//
// The pictures come from the client's own offscreen renderer — the same
// `--dump-menu` shots used to verify a release — and are published to
// `/downloads/shots/<version>/` at release time, next to the manifest and the
// changelog. Nothing here is baked into the static export, so adding pictures
// to a release never needs a website rebuild.

import { useCallback, useEffect, useState } from "react";
import { shotUrl, type ChangeShot } from "../changelog/data";

export default function ShotGallery({
  version,
  shots,
}: {
  version: string;
  shots: ChangeShot[];
}) {
  // Index of the shot shown full screen, or null while browsing thumbnails.
  const [open, setOpen] = useState<number | null>(null);

  const step = useCallback(
    (by: number) =>
      setOpen((i) => (i === null ? i : (i + by + shots.length) % shots.length)),
    [shots.length],
  );

  useEffect(() => {
    if (open === null) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(null);
      else if (e.key === "ArrowRight") step(1);
      else if (e.key === "ArrowLeft") step(-1);
    };
    window.addEventListener("keydown", onKey);
    // Don't let the page scroll behind the viewer.
    const prev = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      window.removeEventListener("keydown", onKey);
      document.body.style.overflow = prev;
    };
  }, [open, step]);

  if (!shots.length) return null;
  const shown = open === null ? null : shots[open];

  return (
    <>
      <div className="shots">
        {shots.map((s, i) => (
          <button
            key={s.src}
            type="button"
            className="shot"
            onClick={() => setOpen(i)}
            aria-label={`Open screenshot: ${s.alt}`}
          >
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img src={shotUrl(version, s.src)} alt={s.alt} loading="lazy" />
            <span className="shot__cap">{s.alt}</span>
          </button>
        ))}
      </div>

      {shown && (
        <div
          className="lightbox"
          role="dialog"
          aria-modal="true"
          aria-label={shown.alt}
          onClick={() => setOpen(null)}
        >
          <div className="lightbox__inner" onClick={(e) => e.stopPropagation()}>
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img src={shotUrl(version, shown.src)} alt={shown.alt} />
            <div className="lightbox__bar">
              <span className="lightbox__v">{version}</span>
              <span className="lightbox__cap">{shown.alt}</span>
              {shots.length > 1 && (
                <span className="lightbox__count">
                  {(open ?? 0) + 1} / {shots.length}
                </span>
              )}
            </div>
          </div>

          {shots.length > 1 && (
            <>
              <button
                type="button"
                className="lightbox__nav prev"
                aria-label="Previous screenshot"
                onClick={(e) => {
                  e.stopPropagation();
                  step(-1);
                }}
              >
                ‹
              </button>
              <button
                type="button"
                className="lightbox__nav next"
                aria-label="Next screenshot"
                onClick={(e) => {
                  e.stopPropagation();
                  step(1);
                }}
              >
                ›
              </button>
            </>
          )}

          <button
            type="button"
            className="lightbox__close"
            aria-label="Close"
            onClick={() => setOpen(null)}
          >
            ✕
          </button>
        </div>
      )}
    </>
  );
}
