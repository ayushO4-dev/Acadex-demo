"use server";

import { cookies, headers } from "next/headers";
import { redirect } from "next/navigation";

import { recordSignIn } from "@/lib/api";
import { PERSONAS } from "@/lib/personas";
import { clientHeadersOf } from "@/lib/request-context";
import { mintToken, SESSION_COOKIE } from "@/lib/session";

/** Signs in as one of the demo's people (and only those). */
export async function signInAs(form: FormData): Promise<void> {
  const email = String(form.get("email") ?? "");
  const returnTo = safeReturnTo(String(form.get("returnTo") ?? ""));
  if (!PERSONAS.some((p) => p.email === email)) redirect("/?signin=failed");

  const { token, expires } = mintToken(`demo|${email}`);
  const requestHeaders = await headers();
  // Records the sign-in in the audit log, and checks the API accepts the token.
  const outcome = await recordSignIn(token, clientHeadersOf(requestHeaders));
  if (outcome !== "ok") redirect(`/?signin=${outcome}`);

  const https = requestHeaders.get("x-forwarded-proto") === "https";
  (await cookies()).set(SESSION_COOKIE, token, { httpOnly: true, sameSite: "lax", secure: https, path: "/", expires });
  redirect(returnTo);
}

/** Only a path on this site, so a crafted link can't send people elsewhere. */
function safeReturnTo(value: string): string {
  return value.startsWith("/") && !value.startsWith("//") && !value.startsWith("/\\") ? value : "/dashboard";
}
