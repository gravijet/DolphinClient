interface LogoProps {
  className?: string;
  animated?: boolean;
}

/** The DolphinClient logo (pixel dolphin in a stone ring). */
export default function Logo({ className, animated = false }: LogoProps) {
  return (
    // eslint-disable-next-line @next/next/no-img-element
    <img
      src="/logo.png"
      alt=""
      aria-hidden="true"
      className={`${className ?? ""}${animated ? " logo--animated" : ""}`}
    />
  );
}
