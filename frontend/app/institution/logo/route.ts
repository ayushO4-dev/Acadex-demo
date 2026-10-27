import { API_URL } from "@/lib/api";

// The university's logo, for the sign-in page: public, briefly cached.
export async function GET() {
  const upstream = await fetch(`${API_URL}/institution/logo`, { next: { revalidate: 60 } }).catch(() => null);
  if (!upstream?.ok) return new Response("No logo", { status: 404 });
  return new Response(upstream.body, {
    headers: {
      "content-type": upstream.headers.get("content-type") ?? "application/octet-stream",
      "cache-control": "public, max-age=300",
      "x-content-type-options": "nosniff",
    },
  });
}
