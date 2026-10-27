import { fetchAsUser, getMe } from "@/lib/acadex";

// A semester's grade card, made by the worker after results are published:
// the signed-in student's own, or (`?student=`) one staff may read, which the
// API's row-level security decides.
export async function GET(request: Request, { params }: { params: Promise<{ semester: string }> }) {
  const { semester } = await params;
  const me = await getMe();
  const asked = new URL(request.url).searchParams.get("student");
  const student = me.student?.id ?? (asked && /^[0-9a-f-]{36}$/.test(asked) ? asked : null);
  if (!student || !/^[1-8]$/.test(semester)) return new Response("Not found", { status: 404 });
  const upstream = await fetchAsUser(`/students/${encodeURIComponent(student)}/grade-cards/${semester}/pdf`);
  if (!upstream.ok) {
    const message =
      upstream.status === 409
        ? "Your grade card is still being prepared. Try again in a minute."
        : "Your grade card isn't available.";
    return new Response(message, { status: upstream.status });
  }
  return new Response(upstream.body, {
    headers: {
      "content-type": "application/pdf",
      "content-disposition": `attachment; filename=grade-card-semester-${semester}.pdf`,
      "cache-control": "private, no-store",
    },
  });
}
