"use client";

import { useEffect, useRef, useState, type ReactNode } from "react";

type Variant = "up" | "down" | "left" | "right" | "zoom" | "fade";

interface RevealProps {
  children: ReactNode;
  /** Animationsrichtung. */
  variant?: Variant;
  /** Verzögerung in ms (für gestaffelte Listen). */
  delay?: number;
  /** Nur einmal animieren (Standard) oder bei jedem Sichtbarwerden. */
  once?: boolean;
  className?: string;
  as?: "div" | "section" | "li" | "article";
  style?: React.CSSProperties;
}

/**
 * Scroll-Reveal: blendet Inhalt sanft ein, sobald er in den Viewport scrollt.
 * Nutzt IntersectionObserver — kein Layout-Thrash, respektiert
 * prefers-reduced-motion (dann sofort sichtbar, ohne Animation).
 */
export default function Reveal({
  children,
  variant = "up",
  delay = 0,
  once = true,
  className = "",
  as = "div",
  style,
}: RevealProps) {
  const ref = useRef<HTMLElement | null>(null);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;

    const reduce =
      typeof window !== "undefined" &&
      window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    if (reduce) {
      setVisible(true);
      return;
    }

    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) {
            setVisible(true);
            if (once) io.unobserve(e.target);
          } else if (!once) {
            setVisible(false);
          }
        }
      },
      { threshold: 0.15, rootMargin: "0px 0px -8% 0px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [once]);

  const Tag = as as any;
  return (
    <Tag
      ref={ref as any}
      className={`reveal reveal--${variant}${visible ? " is-visible" : ""} ${className}`}
      style={{ ...style, transitionDelay: delay ? `${delay}ms` : undefined }}
    >
      {children}
    </Tag>
  );
}
