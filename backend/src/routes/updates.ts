import { FastifyInstance } from "fastify";

/** Routen unter /v1/updates */
export async function updatesRoutes(app: FastifyInstance): Promise<void> {
  // GET /v1/updates/:channel -> Update-Manifest für den Launcher.
  // Muss in Produktion signiert sein (electron-updater prüft Signaturen).
  app.get("/:channel", async (req) => {
    const { channel } = req.params as { channel: string };
    // TODO(M4): echtes, signiertes Manifest mit signierten Artefakt-URLs.
    return {
      channel,
      client: { version: "0.1.0", minecraft: "26.1" },
      launcher: { version: "0.0.1", url: null, signature: null },
    };
  });
}
