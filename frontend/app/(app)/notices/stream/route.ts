import { streamAsUser } from "@/lib/acadex";

// The bell's live updates: the API's server-sent events, passed through.
export async function GET(request: Request) {
  const upstream = await streamAsUser("/notices/stream", request.signal);
  if (!upstream.ok || !upstream.body) return new Response(null, { status: upstream.status || 502 });
  return new Response(upstream.body, {
    headers: { "content-type": "text/event-stream", "cache-control": "no-store, no-transform", "x-accel-buffering": "no" },
  });
}
