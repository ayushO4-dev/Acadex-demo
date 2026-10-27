"use client";

import { useEffect } from "react";

/** Registers the service worker (production builds only: it would fight the dev server). */
export function ServiceWorker() {
  useEffect(() => {
    if (process.env.NODE_ENV !== "production" || !("serviceWorker" in navigator)) return;
    navigator.serviceWorker.register("/sw.js").catch(() => {
      // Not installable then; everything else works the same.
    });
  }, []);
  return null;
}
