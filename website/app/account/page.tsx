import Link from "next/link";
import CosmeticsDashboard from "./CosmeticsDashboard";

export default function AccountPage() {
  return (
    <main>
      <h1>Account</h1>
      <p className="tagline">Verwalte deine Cosmetics.</p>

      <CosmeticsDashboard />

      <p className="honest">
        <Link href="/">Zur Startseite</Link>
      </p>
    </main>
  );
}
