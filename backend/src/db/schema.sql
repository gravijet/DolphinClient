-- DolphinClient — Cosmetics-Schema (PostgreSQL)

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
