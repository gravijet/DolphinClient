"use client";

import { useEffect, useRef } from "react";

/**
 * Dekorativer Hintergrund: aufsteigende Biolumineszenz-„Bubbles" plus ein
 * weicher Cursor-Glow, der der Maus folgt (Parallax-Gefühl). Rein visuell,
 * pointer-events: none, hinter allem. Respektiert reduced-motion (CSS).
 */
export default function BackgroundFX() {
  const glowRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const el = glowRef.current;
    if (!el) return;
    const reduce = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    if (reduce) return;

    let raf = 0;
    let tx = window.innerWidth / 2;
    let ty = window.innerHeight * 0.3;
    let cx = tx;
    let cy = ty;

    const onMove = (e: PointerEvent) => {
      tx = e.clientX;
      ty = e.clientY;
    };
    const loop = () => {
      cx += (tx - cx) * 0.08;
      cy += (ty - cy) * 0.08;
      el.style.setProperty("--mx", `${cx}px`);
      el.style.setProperty("--my", `${cy}px`);
      raf = requestAnimationFrame(loop);
    };
    window.addEventListener("pointermove", onMove, { passive: true });
    raf = requestAnimationFrame(loop);
    return () => {
      window.removeEventListener("pointermove", onMove);
      cancelAnimationFrame(raf);
    };
  }, []);

  // Feste, deterministische Bubble-Konfiguration (kein Math.random beim Render).
  const bubbles = [
    { l: 6, s: 16, d: 22, delay: 0 },
    { l: 18, s: 9, d: 17, delay: 3 },
    { l: 32, s: 22, d: 28, delay: 6 },
    { l: 47, s: 12, d: 19, delay: 1 },
    { l: 58, s: 7, d: 15, delay: 8 },
    { l: 69, s: 18, d: 25, delay: 4 },
    { l: 80, s: 11, d: 20, delay: 2 },
    { l: 90, s: 24, d: 30, delay: 7 },
    { l: 40, s: 6, d: 14, delay: 10 },
    { l: 74, s: 8, d: 16, delay: 5 },
  ];

  return (
    <div className="fx" aria-hidden="true">
      <div className="fx__grid" />
      <div className="fx__glow" ref={glowRef} />
      <div className="fx__bubbles">
        {bubbles.map((b, i) => (
          <span
            key={i}
            className="fx__bubble"
            style={
              {
                left: `${b.l}%`,
                "--sz": `${b.s}px`,
                "--dur": `${b.d}s`,
                animationDelay: `${b.delay}s`,
              } as React.CSSProperties
            }
          />
        ))}
      </div>
    </div>
  );
}
