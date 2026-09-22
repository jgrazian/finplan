import type { NextConfig } from "next";

/**
 * Where `finplan-server` listens. Overridable so a deployment can point the app
 * at a host other than the local dev server.
 */
const API_ORIGIN = process.env.FINPLAN_API_ORIGIN ?? "http://127.0.0.1:8080";

const nextConfig: NextConfig = {
  output: "standalone",
  poweredByHeader: false,
  /**
   * Proxy the API through Next rather than calling the Rust server directly.
   *
   * The session cookie is `SameSite=Lax`, so a browser would drop it on a
   * cross-site XHR from :3000 to :8080. Same-origin `/api/...` requests keep the
   * cookie attached and sidestep CORS entirely.
   */
  async rewrites() {
    return [{ source: "/api/:path*", destination: `${API_ORIGIN}/api/:path*` }];
  },
};

export default nextConfig;
