import { FastifyInstance } from "fastify";
import { getCape, getPlayer, listCapes, setActiveCape } from "../cosmetics/store";

/** Routen unter /v1/cosmetics */
export async function cosmeticsRoutes(app: FastifyInstance): Promise<void> {
  // Alle verfügbaren Capes (für Account-Dashboard / Store).
  app.get("/", async () => ({ capes: await listCapes() }));

  // Welche Cosmetics ein Spieler trägt — wird vom Client beim Start abgefragt.
  app.get("/:uuid", async (req) => {
    const { uuid } = req.params as { uuid: string };
    const player = await getPlayer(uuid);
    const cape = player.activeCapeId ? (await getCape(player.activeCapeId)) ?? null : null;
    return { uuid, cape, items: [] as string[] };
  });

  // Aktive Cape setzen (vom Account-Dashboard). null = keine Cape.
  app.post("/:uuid/active", async (req, reply) => {
    const { uuid } = req.params as { uuid: string };
    const body = (req.body ?? {}) as { capeId: string | null };
    try {
      const player = await setActiveCape(uuid, body.capeId ?? null);
      return { uuid, activeCapeId: player.activeCapeId };
    } catch (e) {
      reply.code(400);
      return { error: (e as Error).message };
    }
  });
}
