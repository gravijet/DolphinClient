import type { Metadata } from "next";
import AdminPortal from "../components/AdminPortal";

// The portal is a static page like every other route — it just reads a JSON
// file that only exists behind the gate. Cloudflare Access authenticates at the
// edge; nginx + the local token gate check the signed assertion again at the
// origin, so this page is unreachable without an approved identity
// (deploy/ZERO-TRUST.md).
export const metadata: Metadata = {
  title: "Admin",
  description: "Operations portal for dolphinclient.de.",
  robots: { index: false, follow: false, nocache: true },
};

export default function AdminPage() {
  return <AdminPortal />;
}
