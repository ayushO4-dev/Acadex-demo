import { randomBytes, timingSafeEqual } from "node:crypto";
import type { NextRequest } from "next/server";

// Browser-facing security for the web app (architecture.md §13): a strict
// Content Security Policy with a fresh nonce per request, the other response
// headers, CSRF protection for every request that changes something, and a
// session bound to the browser that signed in. The Rust API only ever sees
// bearer tokens from this server, so none of this is needed there.

const dev = process.env.NODE_ENV !== "production";

export function newNonce(): string {
  return randomBytes(16).toString("base64");
}

/**
 * Scripts run only with this request's nonce, or when loaded by one that has
 * it ('strict-dynamic': how reCAPTCHA's script arrives). No inline script
 * without the nonce, no eval (except the dev server's hot reload), no
 * framing. Inline style attributes stay allowed: React renders them, and
 * they can't execute code.
 */
export function contentSecurityPolicy(nonce: string, https: boolean): string {
  return [
    "default-src 'self'",
    `script-src 'self' 'nonce-${nonce}' 'strict-dynamic'${dev ? " 'unsafe-eval'" : ""}`,
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data: blob:",
    "font-src 'self' data:",
    `connect-src 'self' https://www.google.com/recaptcha/${dev ? " ws: wss:" : ""}`,
    "frame-src https://www.google.com/recaptcha/ https://recaptcha.google.com/recaptcha/",
    // Spelled out: under 'strict-dynamic' the script-src fallback would refuse it.
    "worker-src 'self'",
    "manifest-src 'self'",
    "frame-ancestors 'none'",
    "base-uri 'self'",
    "form-action 'self'",
    "object-src 'none'",
    ...(https ? ["upgrade-insecure-requests"] : []),
  ].join("; ");
}

/** Headers for every response; HSTS only once the site is served over HTTPS. */
export function securityHeaders(https: boolean): Record<string, string> {
  return {
    "Referrer-Policy": "strict-origin-when-cross-origin",
    "Permissions-Policy": "camera=(), microphone=(), geolocation=(), payment=(), usb=()",
    "X-Content-Type-Options": "nosniff",
    "X-Frame-Options": "DENY",
    "Cross-Origin-Opener-Policy": "same-origin",
    "Cross-Origin-Resource-Policy": "same-origin",
    ...(https ? { "Strict-Transport-Security": "max-age=63072000; includeSubDomains; preload" } : {}),
  };
}

export function isHttps(request: NextRequest): boolean {
  return request.headers.get("x-forwarded-proto") === "https" || request.nextUrl.protocol === "https:";
}

// ── CSRF: SameSite cookies, an Origin check, and a double-submit token ──────

/** __Host- needs Secure, so plain-HTTP local development uses a plain name. */
export function csrfCookieName(https: boolean): string {
  return https ? "__Host-acadex-csrf" : "acadex-csrf";
}
export const CSRF_HEADER = "x-csrf-token";

export function newCsrfToken(): string {
  return randomBytes(24).toString("base64url");
}

/** Requests that change nothing, or that aren't from a browser session. */
export function csrfExempt(request: NextRequest): boolean {
  const method = request.method;
  if (method === "GET" || method === "HEAD" || method === "OPTIONS") return true;
  const path = request.nextUrl.pathname;
  // The test gateway's API is called server to server by the Rust API, with Basic auth.
  return path.startsWith("/mock-gateway/api/");
}

/**
 * A changing request must come from our own origin, and carry the token from
 * the cookie in a header (which another site can neither read nor set).
 * Returns why it's refused, or null.
 */
export function csrfProblem(request: NextRequest, https: boolean): string | null {
  const origin = request.headers.get("origin");
  const allowed = new Set([request.nextUrl.origin]);
  if (process.env.APP_BASE_URL) allowed.add(new URL(process.env.APP_BASE_URL).origin);
  const host = request.headers.get("x-forwarded-host") ?? request.headers.get("host");
  if (host) allowed.add(`${https ? "https" : "http"}://${host}`);
  if (!origin || !allowed.has(origin)) return "cross-origin request";
  const cookie = request.cookies.get(csrfCookieName(https))?.value;
  const header = request.headers.get(CSRF_HEADER);
  if (!cookie || !header || !safeEqual(cookie, header)) return "missing or mismatched CSRF token";
  return null;
}

export function safeEqual(a: string, b: string): boolean {
  const x = Buffer.from(a);
  const y = Buffer.from(b);
  return x.length === y.length && timingSafeEqual(x, y);
}
