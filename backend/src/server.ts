import Fastify from "fastify";
import cors from "@fastify/cors";
import { profileRoutes } from "./routes/profile";
import { cosmeticsRoutes } from "./routes/cosmetics";
import { updatesRoutes } from "./routes/updates";
import { migrate } from "./db/migrate";

const app = Fastify({ logger: true });

async function start(): Promise<void> {
  // CORS, damit die Website (anderer Port) die API im Browser ansprechen darf.
  // In Produktion auf die echte Website-Domain einschränken.
  await app.register(cors, { origin: true });

  app.get("/health", async () => ({
    status: "ok",
    service: "dolphinclient-backend",
  }));

  await app.register(profileRoutes, { prefix: "/v1/profile" });
  await app.register(cosmeticsRoutes, { prefix: "/v1/cosmetics" });
  await app.register(updatesRoutes, { prefix: "/v1/updates" });

  // Schema anlegen + seeden (No-op ohne DATABASE_URL).
  await migrate();

  const port = Number(process.env.PORT ?? 3001);
  const host = process.env.HOST ?? "0.0.0.0";
  await app.listen({ port, host });
  app.log.info(`DolphinClient backend läuft auf ${host}:${port}`);
}

start().catch((err) => {
  app.log.error(err);
  process.exit(1);
});
