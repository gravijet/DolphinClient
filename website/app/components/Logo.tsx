interface LogoProps {
  className?: string;
  animated?: boolean;
}

/** Das DolphinClient-Logo (Pixel-Delfin im Steinring). */
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
