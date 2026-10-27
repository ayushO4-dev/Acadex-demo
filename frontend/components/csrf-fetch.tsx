"use client";

import { useEffect } from "react";

/**
 * The browser half of CSRF's double submit: every same-origin request that
 * changes something (server actions included, which go through fetch) copies
 * the CSRF cookie into a header. Another site can't read the cookie, so it
 * can't forge the header; the middleware compares the two.
 */
export function CsrfFetch({ cookieName }: { cookieName: string }) {
  useEffect(() => {
    const original = window.fetch;
    const patched: typeof window.fetch = (input, init) => {
      // Read without building a Request, which would lock a streamed body.
      const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
      const method = (init?.method ?? (input instanceof Request ? input.method : "GET")).toUpperCase();
      const sameOrigin = new URL(url, location.href).origin === location.origin;
      if (sameOrigin && !["GET", "HEAD", "OPTIONS"].includes(method)) {
        const token = document.cookie
          .split("; ")
          .find((c) => c.startsWith(`${cookieName}=`))
          ?.slice(cookieName.length + 1);
        if (token) {
          const headers = new Headers(init?.headers ?? (input instanceof Request ? input.headers : undefined));
          headers.set("x-csrf-token", decodeURIComponent(token));
          return original(input, { ...init, headers });
        }
      }
      return original(input, init);
    };
    window.fetch = patched;
    return () => {
      window.fetch = original;
    };
  }, [cookieName]);
  return null;
}
