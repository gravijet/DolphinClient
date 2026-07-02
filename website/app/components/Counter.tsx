"use client";

import { useEffect, useRef, useState } from "react";

interface CounterProps {
  to: number;
  from?: number;
  duration?: number;
  decimals?: number;
  prefix?: string;
  suffix?: string;
  /** Tausender-Trennung (de-DE). */
  group?: boolean;
}

/**
 * Zählt eine Zahl hoch, sobald sie ins Sichtfeld kommt (einmalig).
 * easeOutExpo für ein „snappy" Gefühl. Respektiert reduced-motion.
 */
export default function Counter({
  to,
  from = 0,
  duration = 1600,
  decimals = 0,
  prefix = "",
  suffix = "",
  group = false,
}: CounterProps) {
  const ref = useRef<HTMLSpanElement | null>(null);
  const [value, setValue] = useState(from);
  // Track visibility and the current displayed value so the counter can
  // re-animate smoothly when `to` changes (e.g. after an async fetch resolves).
  const visible = useRef(false);
  const current = useRef(from);
  const raf = useRef<number | null>(null);

  const format = (n: number) => {
    const fixed = n.toFixed(decimals);
    if (!group) return fixed;
    const [int, frac] = fixed.split(".");
    const grouped = int.replace(/\B(?=(\d{3})+(?!\d))/g, ".");
    return frac ? `${grouped},${frac}` : grouped;
  };

  useEffect(() => {
    const el = ref.current;
    if (!el) return;

    const reduce = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    if (reduce) {
      current.current = to;
      setValue(to);
      return;
    }

    const animate = () => {
      if (raf.current !== null) cancelAnimationFrame(raf.current);
      const startVal = current.current;
      const start = performance.now();
      const tick = (now: number) => {
        const t = Math.min(1, (now - start) / duration);
        const eased = t === 1 ? 1 : 1 - Math.pow(2, -10 * t); // easeOutExpo
        const v = startVal + (to - startVal) * eased;
        current.current = v;
        setValue(v);
        raf.current = t < 1 ? requestAnimationFrame(tick) : null;
      };
      raf.current = requestAnimationFrame(tick);
    };

    // Already on screen (e.g. `to` changed after first reveal) → retarget now.
    if (visible.current) animate();

    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) {
            visible.current = true;
            animate();
          }
        }
      },
      { threshold: 0.4 },
    );
    io.observe(el);
    return () => {
      io.disconnect();
      if (raf.current !== null) cancelAnimationFrame(raf.current);
    };
  }, [to, from, duration]);

  return (
    <span ref={ref}>
      {prefix}
      {format(value)}
      {suffix}
    </span>
  );
}
