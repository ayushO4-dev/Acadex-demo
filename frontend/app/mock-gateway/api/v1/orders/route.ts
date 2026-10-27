import { authorised, createOrder, enabled } from "@/lib/mock-gateway";

// POST /v1/orders on the test gateway.
export async function POST(request: Request) {
  if (!enabled()) return new Response(null, { status: 404 });
  if (!authorised(request)) return Response.json({ error: "unauthorised" }, { status: 401 });
  const body = (await request.json().catch(() => null)) as { amount?: unknown; receipt?: unknown } | null;
  const amount = Number(body?.amount);
  if (!Number.isInteger(amount) || amount <= 0) return Response.json({ error: "invalid amount" }, { status: 400 });
  return Response.json({ ...createOrder(amount, String(body?.receipt ?? "")), entity: "order" });
}
