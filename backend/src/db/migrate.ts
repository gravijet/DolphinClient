import { getPool } from "./pool";
import { DEFAULT_CAPES } from "../cosmetics/store";

/**
 * Legt das Schema an und seedet die Standard-Capes. No-op ohne DATABASE_URL.
 */
export async function migrate(): Promise<void> {
  const pool = getPool();
  if (!pool) return;

  await pool.query(`
    CREATE TABLE IF NOT EXISTS capes (
      id          TEXT PRIMARY KEY,
      name        TEXT NOT NULL,
      texture_url TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS player_cosmetics (
      uuid           TEXT PRIMARY KEY,
      active_cape_id TEXT REFERENCES capes(id)
    );
    CREATE TABLE IF NOT EXISTS player_owned_capes (
      uuid    TEXT NOT NULL,
      cape_id TEXT NOT NULL REFERENCES capes(id),
      PRIMARY KEY (uuid, cape_id)
    );
  `);

  for (const cape of DEFAULT_CAPES) {
    await pool.query(
      `INSERT INTO capes (id, name, texture_url) VALUES ($1, $2, $3)
       ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, texture_url = EXCLUDED.texture_url`,
      [cape.id, cape.name, cape.textureUrl],
    );
  }
}
