import { FastifyInstance } from "fastify";
import { getCape, getPlayer } from "../cosmetics/store";

/** Routen unter /v1/profile */
export async function profileRoutes(app: FastifyInstance): Promise<void> {
  // GET /v1/profile/:uuid -> Profil + aktive Cosmetics
  app.get("/:uuid", async (req) => {
    const { uuid } = req.params as { uuid: string };
    const player = getPlayer(uuid);
    const cape = player.activeCapeId ? getCape(player.activeCapeId) ?? null : null;
    // TODO(M5+): Identität über Minecraft-Services-Token prüfen; aus DB laden.
    return {
      uuid,
      cosmetics: { cape, ownedCapeIds: player.ownedCapeIds },
      settings: {},
    };
  });
}
