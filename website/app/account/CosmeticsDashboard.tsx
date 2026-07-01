"use client";

import { useEffect, useState } from "react";

interface Cape {
  id: string;
  name: string;
  textureUrl: string;
}

const API_BASE = process.env.NEXT_PUBLIC_API_BASE ?? "http://localhost:3001/v1";

export default function CosmeticsDashboard() {
  const [uuid, setUuid] = useState("");
  const [capes, setCapes] = useState<Cape[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [status, setStatus] = useState("");
  const [statusKind, setStatusKind] = useState<"" | "ok" | "err">("");

  function say(msg: string, kind: "" | "ok" | "err" = "") {
    setStatus(msg);
    setStatusKind(kind);
  }

  // Verfügbare Capes einmalig laden.
  useEffect(() => {
    fetch(`${API_BASE}/cosmetics`)
      .then((r) => r.json())
      .then((d) => setCapes(d.capes ?? []))
      .catch(() => say("Backend nicht erreichbar.", "err"));
  }, []);

  async function loadActive() {
    if (!uuid) return;
    say("Lade …");
    try {
      const r = await fetch(`${API_BASE}/cosmetics/${uuid}`);
      const d = await r.json();
      setActiveId(d.cape?.id ?? null);
      say("Geladen.", "ok");
    } catch {
      say("Konnte aktive Cape nicht laden.", "err");
    }
  }

  async function setActive(capeId: string | null) {
    if (!uuid) {
      say("Bitte zuerst eine UUID eingeben.", "err");
      return;
    }
    try {
      const r = await fetch(`${API_BASE}/cosmetics/${uuid}/active`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ capeId }),
      });
      const d = await r.json();
      if (d.error) {
        say("Fehler: " + d.error, "err");
        return;
      }
      setActiveId(d.activeCapeId ?? null);
      say("Gespeichert.", "ok");
    } catch {
      say("Speichern fehlgeschlagen.", "err");
    }
  }

  return (
    <div className="panel">
      <p className="honest" style={{ margin: 0 }}>
        Dev-Modus: Spieler-UUID eingeben (Web-Login über Microsoft folgt). Die
        Aktionen sprechen direkt mit der Cosmetics-API.
      </p>

      <div className="field">
        <input
          value={uuid}
          onChange={(e) => setUuid(e.target.value.trim())}
          placeholder="Spieler-UUID"
          aria-label="Spieler-UUID"
        />
        <button className="btn ghost" onClick={loadActive}>
          Laden
        </button>
      </div>

      <div className="cape-grid">
        <div
          className={`cape${activeId === null ? " active" : ""}`}
          onClick={() => setActive(null)}
          role="button"
          tabIndex={0}
          onKeyDown={(e) => e.key === "Enter" && setActive(null)}
        >
          <div
            className="cape__swatch"
            style={{ background: "repeating-linear-gradient(45deg,#1a2536,#1a2536 8px,#141d2e 8px,#141d2e 16px)" }}
          />
          <h3>Keine Cape</h3>
          <p>{activeId === null ? "Aktiv" : "Cape ausblenden"}</p>
        </div>

        {capes.map((cape) => (
          <div
            key={cape.id}
            className={`cape${activeId === cape.id ? " active" : ""}`}
            onClick={() => setActive(cape.id)}
            role="button"
            tabIndex={0}
            onKeyDown={(e) => e.key === "Enter" && setActive(cape.id)}
          >
            <div className="cape__swatch" />
            <h3>{cape.name}</h3>
            <p>{activeId === cape.id ? "Aktiv" : "Auswählen"}</p>
          </div>
        ))}
      </div>

      {status && <p className={`status ${statusKind}`}>{status}</p>}
    </div>
  );
}
