import { Pool } from "pg";

/**
 * Liefert einen PostgreSQL-Pool, wenn DATABASE_URL gesetzt ist — sonst null
 * (dann nutzt der Store seinen In-Memory-Fallback).
 */
let pool: Pool | null = null;
let initialized = false;

export function getPool(): Pool | null {
  if (initialized) return pool;
  initialized = true;
  const url = process.env.DATABASE_URL;
  if (url) {
    pool = new Pool({ connectionString: url });
  }
  return pool;
}
