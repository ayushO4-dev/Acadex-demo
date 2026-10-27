import { fetchAsUser } from "@/lib/acadex";

// The academic calendar as an .ics file for Google Calendar, Outlook or a phone.
export async function GET() {
  const upstream = await fetchAsUser("/academic/calendar.ics");
  if (!upstream.ok) return new Response("The calendar couldn't be exported.", { status: upstream.status });
  return new Response(upstream.body, {
    headers: {
      "content-type": "text/calendar; charset=utf-8",
      "content-disposition": upstream.headers.get("content-disposition") ?? "attachment; filename=acadex.ics",
      "cache-control": "private, no-store",
    },
  });
}
