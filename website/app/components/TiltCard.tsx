"use client";

import { useRef, type ReactNode } from "react";

interface TiltCardProps {
  children: ReactNode;
  className?: string;
  /** Maximaler Kippwinkel in Grad. */
  max?: number;
}

/**
 * Karte mit 3D-Pointer-Tilt + „Spotlight", der dem Cursor folgt.
 * Alles über CSS-Variablen — kein Re-Render pro Mausbewegung.
 */
export default function TiltCard({ children, className = "", max = 7 }: TiltCardProps) {
  const ref = useRef<HTMLDivElement | null>(null);

  const onMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const px = (e.clientX - r.left) / r.width;
    const py = (e.clientY - r.top) / r.height;
    el.style.setProperty("--rx", `${(0.5 - py) * max * 2}deg`);
    el.style.setProperty("--ry", `${(px - 0.5) * max * 2}deg`);
    el.style.setProperty("--px", `${px * 100}%`);
    el.style.setProperty("--py", `${py * 100}%`);
  };

  const reset = () => {
    const el = ref.current;
    if (!el) return;
    el.style.setProperty("--rx", "0deg");
    el.style.setProperty("--ry", "0deg");
  };

  return (
    <div
      ref={ref}
      className={`tilt ${className}`}
      onPointerMove={onMove}
      onPointerLeave={reset}
    >
      <div className="tilt__inner">{children}</div>
    </div>
  );
}
