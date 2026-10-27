import { authorised, enabled, refund } from "@/lib/mock-gateway";

// POST /v1/payments/{id}/refund on the test gateway.
export async function POST(request: Request, { params }: { params: Promise<{ id: string }> }) {
  if (!enabled()) return new Response(null, { status: 404 });
  if (!authorised(request)) return Response.json({ error: "unauthorised" }, { status: 401 });
  const { id } = await params;
  const body = (await request.json().catch(() => null)) as { amount?: unknown } | null;
  const result = refund(id, Number(body?.amount), request.headers.get("x-refund-idempotency") ?? "");
  if (!result.ok) return Response.json({ error: { description: result.error } }, { status: result.status });
  const { id: refundId, entity, payment_id, amount, status, created_at } = result.refund;
  return Response.json({ id: refundId, entity, payment_id, amount, status, created_at });
}
