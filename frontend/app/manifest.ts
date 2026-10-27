import type { MetadataRoute } from "next";

// Installable as an app on phones and desktops (architecture.md §8): its own
// window, the Acadex icon, opening at the dashboard.
export default function manifest(): MetadataRoute.Manifest {
  return {
    id: "/",
    name: "Acadex",
    short_name: "Acadex",
    description: "Attendance, examinations, fees and records in one place",
    start_url: "/dashboard",
    scope: "/",
    display: "standalone",
    background_color: "#ffffff",
    theme_color: "#2f5bd9",
    icons: [
      { src: "/icons/icon-192.png", sizes: "192x192", type: "image/png", purpose: "any" },
      { src: "/icons/icon-512.png", sizes: "512x512", type: "image/png", purpose: "any" },
      { src: "/icons/maskable-512.png", sizes: "512x512", type: "image/png", purpose: "maskable" },
    ],
  };
}
