"use client";

// Screenshots for one release. The thumbnails sit in the changelog entry; a
// click opens a full-screen viewer — Esc closes, ← / → step, a click on the
// picture zooms in to 1:1 and drags to pan, and on a phone a swipe moves on.
//
// Most pictures come from the client's own offscreen renderer — the same
// `--dump-menu` shots used to verify a release — and they are published to
// `/downloads/shots/<version>/` at release time, next to the manifest and the
// changelog. Nothing here is baked into the static export, so adding pictures
// to a release never needs a website rebuild.
//
// A UI screenshot is unreadable when it is shrunk to a strip, so the thumbnails
// are deliberately large (one or two per row, never cropped) and the viewer
// fills the screen.

import { useCallback, useEffect, useRef, useState } from "react";
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
  const [zoom, setZoom] = useState(false);
  const touch = useRef<{ x: number; y: number } | null>(null);

  const step = useCallback(
    (by: number) => {
      setZoom(false);
      setOpen((i) => (i === null ? i : (i + by + shots.length) % shots.length));
    },
    [shots.length],
  );

  const close = useCallback(() => {
    setOpen(null);
    setZoom(false);
  }, []);

  useEffect(() => {
    if (open === null) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
      else if (e.key === "ArrowRight") step(1);
      else if (e.key === "ArrowLeft") step(-1);
      else if (e.key === " " || e.key === "Enter") {
        e.preventDefault();
        setZoom((z) => !z);
      }
    };
    window.addEventListener("keydown", onKey);
    // Don't let the page scroll behind the viewer.
    const prev = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      window.removeEventListener("keydown", onKey);
      document.body.style.overflow = prev;
    };
  }, [open, step, close]);

  // Keep the neighbours warm so stepping through feels instant.
  useEffect(() => {
    if (open === null || shots.length < 2) return;
    for (const by of [1, -1]) {
      const n = (open + by + shots.length) % shots.length;
      const img = new Image();
      img.src = shotUrl(version, shots[n].src);
    }
  }, [open, shots, version]);

  if (!shots.length) return null;
  const shown = open === null ? null : shots[open];

  return (
    <>
      <div className={`shots${shots.length === 1 ? " is-single" : ""}`}>
        {shots.map((s, i) => (
          <figure key={s.src} className="shot">
            <button
              type="button"
              className="shot__btn"
              onClick={() => setOpen(i)}
              aria-label={`Open screenshot full screen: ${s.alt}`}
            >
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img src={shotUrl(version, s.src)} alt={s.alt} loading="lazy" decoding="async" />
              <span className="shot__zoom" aria-hidden="true">
                Full screen
              </span>
            </button>
            <figcaption className="shot__cap">{s.alt}</figcaption>
          </figure>
        ))}
      </div>

      {shown && (
        <div
          className="lightbox"
          role="dialog"
          aria-modal="true"
          aria-label={shown.alt}
          onClick={close}
          onTouchStart={(e) => {
            const t = e.touches[0];
            touch.current = { x: t.clientX, y: t.clientY };
          }}
          onTouchEnd={(e) => {
            const start = touch.current;
            touch.current = null;
            if (!start || zoom) return;
            const t = e.changedTouches[0];
            const dx = t.clientX - start.x;
            const dy = t.clientY - start.y;
            if (Math.abs(dx) > 45 && Math.abs(dx) > Math.abs(dy)) step(dx < 0 ? 1 : -1);
            else if (dy > 90 && Math.abs(dy) > Math.abs(dx)) close();
          }}
        >
          <div className="lightbox__top" onClick={(e) => e.stopPropagation()}>
            <span className="lightbox__v">{version}</span>
            {shots.length > 1 && (
              <span className="lightbox__count">
                {(open ?? 0) + 1} / {shots.length}
              </span>
            )}
            <button type="button" className="lightbox__close" aria-label="Close" onClick={close}>
              ✕
            </button>
          </div>

          <div
            className={`lightbox__stage${zoom ? " is-zoom" : ""}`}
            onClick={(e) => {
              e.stopPropagation();
              setZoom((z) => !z);
            }}
          >
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img src={shotUrl(version, shown.src)} alt={shown.alt} />
          </div>

          <div className="lightbox__bottom" onClick={(e) => e.stopPropagation()}>
            <p className="lightbox__cap">{shown.alt}</p>
            {shots.length > 1 && (
              <div className="lightbox__strip">
                {shots.map((s, i) => (
                  <button
                    key={s.src}
                    type="button"
                    className={i === open ? "is-on" : undefined}
                    aria-label={`Show screenshot ${i + 1}: ${s.alt}`}
                    onClick={() => {
                      setZoom(false);
                      setOpen(i);
                    }}
                  >
                    {/* eslint-disable-next-line @next/next/no-img-element */}
                    <img src={shotUrl(version, s.src)} alt="" loading="lazy" />
                  </button>
                ))}
              </div>
            )}
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
        </div>
      )}
    </>
  );
}
