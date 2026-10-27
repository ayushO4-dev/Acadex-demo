import type { ClientHeaders } from "@/lib/api";

/**
 * The browser's address is the last X-Forwarded-For entry: the one our own
 * front end appended (Cloud Run's; locally Next.js fills it from the socket
 * when it's absent). Anything before it is whatever the browser sent.
 */
export function clientHeadersOf(headers: Headers): ClientHeaders {
  return {
    ip: headers.get("x-forwarded-for")?.split(",").at(-1)?.trim() || null,
    userAgent: headers.get("user-agent"),
  };
}
