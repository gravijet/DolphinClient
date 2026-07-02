import { FastifyInstance } from "fastify";

/** Routen unter /v1/updates */
export async function updatesRoutes(app: FastifyInstance): Promise<void> {
  // JSON-Manifest (Übersicht für Website/Client-Mod).
  app.get("/:channel", async (req) => {
    const { channel } = req.params as { channel: string };
    return {
      channel,
      client: { version: "0.1.0", minecraft: "26.1" },
      launcher: { version: "0.2.3" },
    };
  });

  // Feed für electron-updater (generischer Provider lädt <feed>/latest.yml).
  // In Produktion wird diese Datei von electron-builder beim Release erzeugt und
  // statisch (mit den echten sha512/size der Artefakte) ausgeliefert. Hier nur
  // ein wohlgeformter Platzhalter, bis es echte signierte Builds gibt.
  app.get("/:channel/latest.yml", async (req, reply) => {
    const { channel } = req.params as { channel: string };
    const version = "0.0.1";
    const file = `DolphinClient-Setup-${version}.exe`;
    const yml = [
      `version: ${version}`,
      `path: ${file}`,
      `sha512: PLACEHOLDER`,
      `releaseDate: '${new Date().toISOString()}'`,
      `# channel: ${channel} — echte Werte erzeugt electron-builder beim Release.`,
      "",
    ].join("\n");
    reply.header("Content-Type", "text/yaml");
    return yml;
  });
}
