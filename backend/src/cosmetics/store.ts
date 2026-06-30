/**
 * Cosmetics-Datenhaltung.
 *
 * Stand M5: In-Memory-Store, damit die Endpunkte real funktionieren. In
 * Produktion ersetzen durch PostgreSQL (Besitz/Aktiv-Status) + Objekt-Storage/CDN
 * für die Texturen, und echte Entitlements statt "jeder besitzt alles".
 */

export interface Cape {
  id: string;
  name: string;
  textureUrl: string;
}

export interface PlayerCosmetics {
  uuid: string;
  activeCapeId: string | null;
  ownedCapeIds: string[];
}

const CDN_BASE = process.env.CDN_BASE_URL ?? "https://cdn.dolphinclient.example/capes";

const CAPES: Cape[] = [
  { id: "dolphin", name: "Dolphin Cape", textureUrl: `${CDN_BASE}/dolphin.png` },
  { id: "ocean", name: "Ocean Cape", textureUrl: `${CDN_BASE}/ocean.png` },
];

const players = new Map<string, PlayerCosmetics>();

export function listCapes(): Cape[] {
  return CAPES;
}

export function getCape(id: string): Cape | undefined {
  return CAPES.find((c) => c.id === id);
}

export function getPlayer(uuid: string): PlayerCosmetics {
  let player = players.get(uuid);
  if (!player) {
    // Platzhalter: jeder "besitzt" vorerst alle Capes (bis es Käufe gibt).
    player = { uuid, activeCapeId: null, ownedCapeIds: CAPES.map((c) => c.id) };
    players.set(uuid, player);
  }
  return player;
}

export function setActiveCape(uuid: string, capeId: string | null): PlayerCosmetics {
  const player = getPlayer(uuid);
  if (capeId !== null && !getCape(capeId)) {
    throw new Error("Unbekannte Cape-ID");
  }
  if (capeId !== null && !player.ownedCapeIds.includes(capeId)) {
    throw new Error("Cape nicht im Besitz");
  }
  player.activeCapeId = capeId;
  return player;
}
