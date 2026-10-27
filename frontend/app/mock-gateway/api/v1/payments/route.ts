import { authorised, enabled, listPayments } from "@/lib/mock-gateway";

// GET /v1/payments?from&to&count&skip on the test gateway: what reconciliation reads.
export async function GET(request: Request) {
  if (!enabled()) return new Response(null, { status: 404 });
  if (!authorised(request)) return Response.json({ error: "unauthorised" }, { status: 401 });
  const q = new URL(request.url).searchParams;
  const num = (k: string, d: number) => (Number.isFinite(Number(q.get(k))) && q.get(k) ? Number(q.get(k)) : d);
  const items = listPayments(num("from", 0), num("to", Number.MAX_SAFE_INTEGER), Math.min(num("count", 10), 100), num("skip", 0));
  return Response.json({ entity: "collection", count: items.length, items });
}
