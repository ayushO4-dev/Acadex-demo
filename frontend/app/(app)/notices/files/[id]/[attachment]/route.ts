import { fetchAsUser } from "@/lib/acadex";

// A notice's attachment, for whoever the notice is visible to.
export async function GET(_request: Request, { params }: { params: Promise<{ id: string; attachment: string }> }) {
  const { id, attachment } = await params;
  const upstream = await fetchAsUser(`/notices/${encodeURIComponent(id)}/attachments/${encodeURIComponent(attachment)}`);
  if (!upstream.ok) return new Response("That file isn't available.", { status: upstream.status });
  const headers = new Headers({ "cache-control": "private, no-store", "x-content-type-options": "nosniff" });
  for (const name of ["content-type", "content-disposition"]) {
    const value = upstream.headers.get(name);
    if (value) headers.set(name, value);
  }
  return new Response(upstream.body, { headers });
}
