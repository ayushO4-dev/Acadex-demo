import { fetchAsUser } from "@/lib/acadex";

// Streams a receipt PDF from the API. The API checks it's the signed-in
// student's own receipt; nothing is cached on the way.
export async function GET(_request: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const upstream = await fetchAsUser(`/fees/receipts/${encodeURIComponent(id)}/pdf`);
  if (!upstream.ok) {
    const message =
      upstream.status === 409
        ? "This receipt is still being generated. Try again in a moment."
        : upstream.status === 404
          ? "No such receipt."
          : "The receipt couldn't be fetched. Please try again.";
    return new Response(message, { status: upstream.status, headers: { "content-type": "text/plain; charset=utf-8" } });
  }
  return new Response(upstream.body, {
    headers: {
      "content-type": "application/pdf",
      "content-disposition": upstream.headers.get("content-disposition") ?? "attachment",
      "cache-control": "private, no-store",
    },
  });
}
