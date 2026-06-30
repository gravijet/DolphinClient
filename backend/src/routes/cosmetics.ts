import { FastifyInstance } from "fastify";

/** Routen unter /v1/cosmetics */
export async function cosmeticsRoutes(app: FastifyInstance): Promise<void> {
  // GET /v1/cosmetics/:uuid -> welche Cosmetics ein Spieler trägt.
  // Wird vom Client beim Start für jeden sichtbaren Spieler abgefragt.
  app.get("/:uuid", async (req) => {
    const { uuid } = req.params as { uuid: string };
    // TODO(M5): aktive Cape-/Item-URLs aus DB + CDN auflösen.
    return { uuid, cape: null, items: [] as string[] };
  });
}
