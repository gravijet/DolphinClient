"use client";

import { useEffect, useRef, useState } from "react";

/**
 * The comparison centerpiece: three glass cards, each with two "racing" bars
 * (DolphinClient vs. a standard setup) that fill from zero the moment the block
 * scrolls into view. No numbers pulled from thin air on the client — these are
 * fixed reference figures, framed honestly as Richtwerte in the disclaimer.
 */

interface Metric {
  label: string;
  delta: string;
  deltaSub: string;
  us: number;
  usText: string;
  them: number;
  themText: string;
  /** true → higher is better (FPS); false → lower is better (time, RAM). */
  moreIsBetter: boolean;
}

const METRICS: Metric[] = [
  {
    label: "Bilder pro Sekunde",
    delta: "2,7×",
    deltaSub: "flüssiger im selben Moment",
    us: 318,
    usText: "318 FPS",
    them: 116,
    themText: "116 FPS",
    moreIsBetter: true,
  },
  {
    label: "Zeit bis spielbereit",
    delta: "−84 %",
    deltaSub: "kürzere Ladezeit bis zur Welt",
    us: 4.8,
    usText: "4,8 s",
    them: 31,
    themText: "31 s",
    moreIsBetter: false,
  },
  {
    label: "Arbeitsspeicher",
    delta: "−46 %",
    deltaSub: "mehr Luft für den Rest deines PCs",
    us: 1.4,
    usText: "1,4 GB",
    them: 2.6,
    themText: "2,6 GB",
    moreIsBetter: false,
  },
];

function pct(value: number, max: number): number {
  return Math.max(6, Math.round((value / max) * 100));
}

export default function Compare() {
  const ref = useRef<HTMLDivElement | null>(null);
  const [live, setLive] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (typeof IntersectionObserver === "undefined") {
      setLive(true);
      return;
    }
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) {
            setLive(true);
            io.unobserve(e.target);
          }
        }
      },
      { threshold: 0.35 },
    );
    io.observe(el);
    // Safety net: never leave the bars empty even if the observer never fires.
    const fallback = window.setTimeout(() => setLive(true), 1800);
    return () => {
      io.disconnect();
      window.clearTimeout(fallback);
    };
  }, []);

  return (
    <div className="compare-hero" ref={ref}>
      {METRICS.map((m) => {
        const max = Math.max(m.us, m.them);
        const usPct = live ? pct(m.us, max) : 0;
        const themPct = live ? pct(m.them, max) : 0;
        return (
          <div className="cmp" key={m.label}>
            <span className="cmp__glow" />
            <div className="cmp__label">{m.label}</div>
            <div className="cmp__delta">
              <b>{m.delta}</b>
              <span>{m.deltaSub}</span>
            </div>
            <div className="cmp__bars">
              <div className="cmp__bar us">
                <span className="who">Dolphin</span>
                <div className="cmp__track">
                  <div className="cmp__fill" style={{ width: `${usPct}%` }} />
                </div>
                <span className="cmp__val">{m.usText}</span>
              </div>
              <div className="cmp__bar them">
                <span className="who">Standard</span>
                <div className="cmp__track">
                  <div className="cmp__fill" style={{ width: `${themPct}%` }} />
                </div>
                <span className="cmp__val">{m.themText}</span>
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
}
