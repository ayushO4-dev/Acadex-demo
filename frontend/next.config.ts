import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // A self-contained server (server.js plus only the dependencies it uses),
  // so the container image needs no node_modules install.
  output: "standalone",
  // Don't advertise the framework.
  poweredByHeader: false,
  experimental: {
    // Certificates and documents are uploaded through server actions; the API
    // itself caps them at 5 MB.
    serverActions: { bodySizeLimit: "6mb" },
  },
};

export default nextConfig;
