import { createPrivateKey, createSign, type KeyObject } from "node:crypto";

import { cookies } from "next/headers";

// The demo's sign-in, in place of Auth0. Signing in as a persona mints a
// short-lived RS256 token with the private key `just setup` generated; the
// API trusts only the matching public key, with the issuer, audience and
// algorithm pinned, exactly as it checks Auth0's tokens in the full product.
// The token lives in an httpOnly cookie, so page scripts can't read it.

export const SESSION_COOKIE = "acadex_session";
export const ISSUER = "acadex-demo";
export const AUDIENCE = "acadex-api";
const KEY_ID = "acadex-demo-1";
const TTL_SECONDS = 8 * 60 * 60;

let signingKey: KeyObject | null = null;

function key(): KeyObject {
  if (!signingKey) {
    const pem = Buffer.from(process.env.DEMO_JWT_PRIVATE_KEY ?? "", "base64").toString("utf8");
    if (!pem.includes("PRIVATE KEY")) throw new Error("DEMO_JWT_PRIVATE_KEY is not set: run `just setup`");
    signingKey = createPrivateKey(pem);
  }
  return signingKey;
}

const b64url = (data: Buffer | string) => Buffer.from(data).toString("base64url");

/** A token for `subject`, as Auth0 would issue it. */
export function mintToken(subject: string): { token: string; expires: Date } {
  const now = Math.floor(Date.now() / 1000);
  const header = b64url(JSON.stringify({ alg: "RS256", typ: "JWT", kid: KEY_ID }));
  const payload = b64url(JSON.stringify({ iss: ISSUER, aud: AUDIENCE, sub: subject, iat: now, exp: now + TTL_SECONDS }));
  const signature = createSign("RSA-SHA256").update(`${header}.${payload}`).sign(key()).toString("base64url");
  return { token: `${header}.${payload}.${signature}`, expires: new Date((now + TTL_SECONDS) * 1000) };
}

export type Session = { token: string; subject: string };

/** The signed-in person's session, or null when signed out or expired. */
export async function getSession(): Promise<Session | null> {
  const token = (await cookies()).get(SESSION_COOKIE)?.value;
  if (!token) return null;
  try {
    const claims = JSON.parse(Buffer.from(token.split(".")[1], "base64url").toString()) as { sub: string; exp: number };
    // The API verifies the signature; here only "is it still current?" matters.
    if (claims.exp * 1000 <= Date.now()) return null;
    return { token, subject: claims.sub };
  } catch {
    return null;
  }
}
