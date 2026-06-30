import type { ReactNode } from "react";
import "./globals.css";

export const metadata = {
  title: "DolphinClient",
  description: "Performance-orientierter Minecraft-Client für 26.1.",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="de">
      <body>
        {children}
        <footer
          style={{
            maxWidth: 880,
            margin: "0 auto",
            padding: "2rem 1.5rem",
            opacity: 0.5,
            fontSize: "0.85rem",
          }}
        >
          Nicht mit Mojang oder Microsoft verbunden. Minecraft ist eine Marke von
          Mojang Synergies AB.
        </footer>
      </body>
    </html>
  );
}
