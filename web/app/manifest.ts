import type { MetadataRoute } from "next";

/**
 * What makes FinPlan installable (spec 19, phase 5): Safari deletes the
 * storage of a site not visited for 7 days unless it is an installed app, and
 * an installed app opens offline once the service worker has cached the shell.
 * Colours are the light palette's `--color-bg` and `--color-accent`.
 */
export default function manifest(): MetadataRoute.Manifest {
  return {
    name: "FinPlan",
    short_name: "FinPlan",
    description: "Monte Carlo retirement simulation",
    start_url: "/",
    scope: "/",
    display: "standalone",
    background_color: "#f8f9fa",
    theme_color: "#2f5f9e",
    icons: [
      { src: "/icon.svg", sizes: "any", type: "image/svg+xml", purpose: "any" },
    ],
  };
}
