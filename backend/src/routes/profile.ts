import { FastifyInstance } from "fastify";

/** Routen unter /v1/profile */
export async function profileRoutes(app: FastifyInstance): Promise<void> {
  // GET /v1/profile/:uuid -> Profil + aktive Cosmetics + Einstellungen
  app.get("/:uuid", async (req) => {
    const { uuid } = req.params as { uuid: string };
    // TODO(M5): aus PostgreSQL laden; Identität über MC-Services-Token prüfen.
    return {
      uuid,
      cosmetics: { cape: null },
      settings: {},
    };
  });
}
