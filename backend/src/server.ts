import Fastify from "fastify";
import { profileRoutes } from "./routes/profile";
import { cosmeticsRoutes } from "./routes/cosmetics";
import { updatesRoutes } from "./routes/updates";

const app = Fastify({ logger: true });

app.get("/health", async () => ({
  status: "ok",
  service: "dolphinclient-backend",
}));

app.register(profileRoutes, { prefix: "/v1/profile" });
app.register(cosmeticsRoutes, { prefix: "/v1/cosmetics" });
app.register(updatesRoutes, { prefix: "/v1/updates" });

const port = Number(process.env.PORT ?? 3001);
const host = process.env.HOST ?? "0.0.0.0";

app
  .listen({ port, host })
  .then((addr) => app.log.info(`DolphinClient backend läuft auf ${addr}`))
  .catch((err) => {
    app.log.error(err);
    process.exit(1);
  });
