import { getPool } from "../db/pool";

/**
 * Cosmetics-Datenhaltung. Nutzt PostgreSQL, wenn DATABASE_URL gesetzt ist,
 * sonst einen In-Memory-Fallback (für Dev ohne DB). Entitlements sind vorerst
 * "jeder besitzt alles" — bis es Käufe gibt.
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

export const DEFAULT_CAPES: Cape[] = [
  { id: "dolphin", name: "Dolphin Cape", textureUrl: `${CDN_BASE}/dolphin.png` },
  { id: "ocean", name: "Ocean Cape", textureUrl: `${CDN_BASE}/ocean.png` },
];

// In-Memory-Fallback.
const memPlayers = new Map<string, PlayerCosmetics>();

export async function listCapes(): Promise<Cape[]> {
  const pool = getPool();
  if (!pool) return DEFAULT_CAPES;
  const res = await pool.query("SELECT id, name, texture_url FROM capes ORDER BY id");
  return res.rows.map((r) => ({ id: r.id, name: r.name, textureUrl: r.texture_url }));
}

export async function getCape(id: string): Promise<Cape | undefined> {
  return (await listCapes()).find((c) => c.id === id);
}

export async function getPlayer(uuid: string): Promise<PlayerCosmetics> {
  const pool = getPool();
  if (!pool) {
    let player = memPlayers.get(uuid);
    if (!player) {
      player = { uuid, activeCapeId: null, ownedCapeIds: DEFAULT_CAPES.map((c) => c.id) };
      memPlayers.set(uuid, player);
    }
    return player;
  }

  const existing = await pool.query(
    "SELECT active_cape_id FROM player_cosmetics WHERE uuid = $1",
    [uuid],
  );

  if (existing.rowCount === 0) {
    // Neuer Spieler: anlegen und (Platzhalter) alle Capes gewähren.
    const all = await listCapes();
    await pool.query(
      "INSERT INTO player_cosmetics (uuid, active_cape_id) VALUES ($1, NULL) ON CONFLICT DO NOTHING",
      [uuid],
    );
    for (const cape of all) {
      await pool.query(
        "INSERT INTO player_owned_capes (uuid, cape_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        [uuid, cape.id],
      );
    }
    return { uuid, activeCapeId: null, ownedCapeIds: all.map((c) => c.id) };
  }

  const owned = await pool.query(
    "SELECT cape_id FROM player_owned_capes WHERE uuid = $1",
    [uuid],
  );
  return {
    uuid,
    activeCapeId: existing.rows[0].active_cape_id ?? null,
    ownedCapeIds: owned.rows.map((r) => r.cape_id),
  };
}

export async function setActiveCape(
  uuid: string,
  capeId: string | null,
): Promise<PlayerCosmetics> {
  const player = await getPlayer(uuid);
  if (capeId !== null) {
    const cape = await getCape(capeId);
    if (!cape) throw new Error("Unbekannte Cape-ID");
    if (!player.ownedCapeIds.includes(capeId)) throw new Error("Cape nicht im Besitz");
  }

  const pool = getPool();
  if (pool) {
    await pool.query("UPDATE player_cosmetics SET active_cape_id = $2 WHERE uuid = $1", [
      uuid,
      capeId,
    ]);
  }
  player.activeCapeId = capeId;
  return player;
}
