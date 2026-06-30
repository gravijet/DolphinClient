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

  // Verfügbare Capes einmalig laden.
  useEffect(() => {
    fetch(`${API_BASE}/cosmetics`)
      .then((r) => r.json())
      .then((d) => setCapes(d.capes ?? []))
      .catch(() => setStatus("Backend nicht erreichbar."));
  }, []);

  async function loadActive() {
    if (!uuid) return;
    setStatus("Lade …");
    try {
      const r = await fetch(`${API_BASE}/cosmetics/${uuid}`);
      const d = await r.json();
      setActiveId(d.cape?.id ?? null);
      setStatus("");
    } catch {
      setStatus("Konnte aktive Cape nicht laden.");
    }
  }

  async function setActive(capeId: string | null) {
    if (!uuid) {
      setStatus("Bitte zuerst eine UUID eingeben.");
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
        setStatus("Fehler: " + d.error);
        return;
      }
      setActiveId(d.activeCapeId ?? null);
      setStatus("Gespeichert.");
    } catch {
      setStatus("Speichern fehlgeschlagen.");
    }
  }

  return (
    <div>
      <p className="honest">
        Dev-Modus: Spieler-UUID eingeben (Web-Login über Microsoft folgt). Die
        Aktionen sprechen direkt mit der Cosmetics-API.
      </p>

      <div className="cta">
        <input
          value={uuid}
          onChange={(e) => setUuid(e.target.value.trim())}
          placeholder="Spieler-UUID"
          style={{
            padding: "0.6rem 0.9rem",
            borderRadius: 8,
            border: "1px solid #334155",
            background: "#0f1420",
            color: "#e8edf7",
            minWidth: 320,
          }}
        />
        <button className="btn ghost" onClick={loadActive} style={{ border: 0 }}>
          Laden
        </button>
      </div>

      <section className="features">
        <div
          onClick={() => setActive(null)}
          style={{ cursor: "pointer", outline: activeId === null ? "2px solid #4aa3ff" : "none" }}
        >
          <h3>Keine Cape</h3>
          <p>Cape ausblenden.</p>
        </div>
        {capes.map((cape) => (
          <div
            key={cape.id}
            onClick={() => setActive(cape.id)}
            style={{ cursor: "pointer", outline: activeId === cape.id ? "2px solid #4aa3ff" : "none" }}
          >
            <h3>{cape.name}</h3>
            <p>{activeId === cape.id ? "Aktiv" : "Auswählen"}</p>
          </div>
        ))}
      </section>

      {status && <p className="honest">{status}</p>}
    </div>
  );
}
