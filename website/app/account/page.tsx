import Link from "next/link";

export default function AccountPage() {
  return (
    <main>
      <h1>Account</h1>
      <p className="tagline">Verwalte deine Cosmetics und Einstellungen.</p>
      <p className="honest">
        Login über Microsoft (wie im Launcher). Das Dashboard zum Verwalten der
        Capes/Cosmetics gegen die Backend-API folgt in M6.
      </p>

      <div className="cta">
        <a className="btn" href="#" aria-disabled>
          Mit Microsoft anmelden — bald
        </a>
        <Link className="btn ghost" href="/">
          Zur Startseite
        </Link>
      </div>
    </main>
  );
}
