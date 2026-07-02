interface LogoProps {
  className?: string;
  animated?: boolean;
}

/** DolphinClient-Wortmarke (nur das Symbol). */
export default function Logo({ className, animated = false }: LogoProps) {
  return (
    <svg
      viewBox="0 0 48 48"
      aria-hidden="true"
      xmlns="http://www.w3.org/2000/svg"
      className={`${className ?? ""}${animated ? " logo--animated" : ""}`}
    >
      <defs>
        <linearGradient id="dcLogo" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#38e1c4" />
          <stop offset="0.5" stopColor="#4ac6ff" />
          <stop offset="1" stopColor="#7c8bff" />
        </linearGradient>
      </defs>
      <rect width="48" height="48" rx="13" fill="url(#dcLogo)" />
      <path
        fill="#ffffff"
        d="M13 33c7-9 13-15 21-18-4 6-7 11-7 17-3-2-9-2-13 2-.4-.4-1-.6-1-1Z"
      />
      <path
        d="M11 36c5-3 9 2 14-1 5-3 9 2 13-1"
        fill="none"
        stroke="#ffffff"
        strokeOpacity="0.85"
        strokeWidth="2.4"
        strokeLinecap="round"
      />
    </svg>
  );
}
