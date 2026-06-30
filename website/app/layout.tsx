import type { ReactNode } from "react";
import "./globals.css";

export const metadata = {
  title: "DolphinClient",
  description: "Performance-orientierter Minecraft-Client für 26.1.",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="de">
      <body>{children}</body>
    </html>
  );
}
