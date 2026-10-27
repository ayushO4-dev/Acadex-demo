import { cookies } from "next/headers";
import { type NextRequest, NextResponse } from "next/server";

import { recordSignOut } from "@/lib/api";
import { clientHeadersOf } from "@/lib/request-context";
import { getSession, SESSION_COOKIE } from "@/lib/session";

/** Signing out: logged while the token still works, then the cookie goes. */
export async function GET(request: NextRequest) {
  const session = await getSession();
  if (session) await recordSignOut(session.token, clientHeadersOf(request.headers));
  (await cookies()).delete(SESSION_COOKIE);
  return NextResponse.redirect(new URL("/", process.env.APP_BASE_URL ?? request.url));
}
