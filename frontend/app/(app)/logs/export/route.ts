import { fetchAsUser } from "@/lib/acadex";

import { FILTER_PARAMS } from "../labels";

// The Logs page's CSV export, with the page's filters. The API checks whose
// rows the user may see and logs the export itself.
export async function GET(request: Request) {
  const from = new URL(request.url).searchParams;
  const query = new URLSearchParams();
  for (const key of FILTER_PARAMS) {
    const value = from.get(key);
    if (value) query.set(key, value);
  }
  const upstream = await fetchAsUser(`/audit/logs/export?${query}`);
  if (!upstream.ok) {
    return new Response("The export couldn't be made. Check the filters and try again.", {
      status: upstream.status,
      headers: { "content-type": "text/plain; charset=utf-8" },
    });
  }
  return new Response(upstream.body, {
    headers: {
      "content-type": "text/csv; charset=utf-8",
      "content-disposition": upstream.headers.get("content-disposition") ?? "attachment; filename=\"acadex-logs.csv\"",
      "cache-control": "private, no-store",
    },
  });
}
