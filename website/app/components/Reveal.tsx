"use client";

import { useEffect, useRef, useState, type ReactNode } from "react";

type Variant = "up" | "down" | "left" | "right" | "zoom" | "fade";

interface RevealProps {
  children: ReactNode;
  /** Animation direction. */
  variant?: Variant;
  /** Delay in ms (for staggered lists). */
  delay?: number;
  /** Animate once (default) or every time it becomes visible. */
  once?: boolean;
  className?: string;
  as?: "div" | "section" | "li" | "article";
  style?: React.CSSProperties;
  id?: string;
}

/**
 * Scroll reveal: gently fades content in as it scrolls into the viewport.
 * Uses IntersectionObserver — no layout thrash, respects prefers-reduced-motion
 * (then visible immediately, without animation).
 */
export default function Reveal({
  children,
  variant = "up",
  delay = 0,
  once = true,
  className = "",
  as = "div",
  style,
  id,
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

    if (typeof IntersectionObserver === "undefined") {
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
    // Safety net: if the observer never delivers (rare, or a very tall
    // element already spanning the viewport), reveal anyway so content is
    // never permanently stuck at opacity 0.
    const fallback = window.setTimeout(() => setVisible(true), 2600);
    return () => {
      io.disconnect();
      window.clearTimeout(fallback);
    };
  }, [once]);

  const Tag = as as any;
  return (
    <Tag
      ref={ref as any}
      id={id}
      className={`reveal reveal--${variant}${visible ? " is-visible" : ""} ${className}`}
      style={{ ...style, transitionDelay: delay ? `${delay}ms` : undefined }}
    >
      {children}
    </Tag>
  );
}
