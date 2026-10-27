import { NextResponse, type NextRequest } from "next/server";

import {
  contentSecurityPolicy,
  csrfCookieName,
  csrfExempt,
  csrfProblem,
  isHttps,
  newCsrfToken,
  newNonce,
  securityHeaders,
} from "@/lib/security";

// Every request: the CSRF check, then the page with this request's CSP nonce
// and the other security headers. (Signing in and out are a server action and
// a route handler; see lib/session.ts.)
export async function middleware(request: NextRequest) {
  const https = isHttps(request);

  if (!csrfExempt(request)) {
    const problem = csrfProblem(request, https);
    if (problem) {
      console.warn(`[security] refused ${request.method} ${request.nextUrl.pathname}: ${problem}`);
      return secure(new NextResponse("Request refused: it didn't come from an Acadex page.", { status: 403 }), request, https, newNonce());
    }
  }

  // The nonce travels to the page in the request headers: Next.js stamps it on
  // its own scripts, and the root layout passes it to the theme script.
  const nonce = newNonce();
  const requestHeaders = new Headers(request.headers);
  requestHeaders.set("x-nonce", nonce);
  requestHeaders.set("content-security-policy", contentSecurityPolicy(nonce, https));
  return secure(NextResponse.next({ request: { headers: requestHeaders } }), request, https, nonce);
}

/** The CSP and other headers, and the CSRF cookie if the browser has none yet. */
function secure(response: NextResponse, request: NextRequest, https: boolean, nonce: string): NextResponse {
  response.headers.set("Content-Security-Policy", contentSecurityPolicy(nonce, https));
  for (const [key, value] of Object.entries(securityHeaders(https))) response.headers.set(key, value);
  const name = csrfCookieName(https);
  if (!request.cookies.has(name)) {
    // Readable by our own script (it copies it into a header); SameSite=Strict.
    response.cookies.set(name, newCsrfToken(), { httpOnly: false, sameSite: "strict", secure: https, path: "/" });
  }
  return response;
}

export const config = {
  // Node.js runtime for node:crypto and the OpenTelemetry instrumentation.
  runtime: "nodejs",
  matcher: ["/((?!_next/static|_next/image|favicon.ico|sitemap.xml|robots.txt).*)"],
};
