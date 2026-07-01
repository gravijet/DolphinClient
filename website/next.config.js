/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  // Statischer Export (out/) — direkt von Nginx ausgeliefert, kein Node-Runtime
  // für die Website nötig. Der Account-Bereich spricht die API im Browser an.
  output: "export",
  trailingSlash: true,
  images: { unoptimized: true },
};

module.exports = nextConfig;
