import Link from "next/link";

export default function DownloadPage() {
  return (
    <main>
      <h1>Herunterladen</h1>
      <p className="tagline">DolphinClient für Minecraft 26.1.</p>
      <p className="honest">
        Du brauchst ein gültiges Minecraft-Konto (Microsoft). Der Launcher lädt
        die Original-Spieldateien direkt von Mojang.
      </p>

      <div className="cta">
        <a className="btn" href="#" aria-disabled>
          Windows (.exe) — bald
        </a>
        <a className="btn" href="#" aria-disabled>
          macOS (.dmg) — bald
        </a>
        <a className="btn ghost" href="#" aria-disabled>
          Linux (.AppImage) — bald
        </a>
      </div>

      <p className="honest">
        Die Download-Links werden vom signierten Update-Feed des Backends
        befüllt (M4). <Link href="/">Zur Startseite</Link>
      </p>
    </main>
  );
}
