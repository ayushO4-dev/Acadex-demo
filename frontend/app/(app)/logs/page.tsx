import { Download } from "lucide-react";
import Link from "next/link";

import { ApiProblem } from "@/components/api-problem";
import { PageHeader } from "@/components/page-header";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { UrlInput } from "@/components/url-input";
import { UrlSelect } from "@/components/url-select";
import { getLogs, getMe } from "@/lib/acadex";
import type { LogEntry, LogPage } from "@/lib/api";

import { ACTIONS, CATEGORIES, FILTER_PARAMS, OUTCOMES, ROLES, SYSTEM_ACTORS } from "./labels";

export const dynamic = "force-dynamic";

const TIME = new Intl.DateTimeFormat("en-IN", {
  dateStyle: "medium",
  timeStyle: "short",
  timeZone: "Asia/Kolkata",
});

type Params = Partial<Record<(typeof FILTER_PARAMS)[number] | "before", string>>;

export default async function LogsPage({ searchParams }: { searchParams: Promise<Params> }) {
  const params = await searchParams;
  const me = await getMe();
  if (!me.permissions.includes("audit:read_all")) {
    return (
      <>
        <PageHeader title="Logs" />
        <p className="text-muted-foreground">The audit log is for administration.</p>
      </>
    );
  }

  // The filters, as they go to the API and into every link on this page.
  const filters = new URLSearchParams();
  for (const key of FILTER_PARAMS) {
    const value = params[key];
    if (value && value !== "all") filters.set(key, value);
  }
  const withFilters = (extra?: Record<string, string>) => {
    const query = new URLSearchParams(filters);
    for (const [k, v] of Object.entries(extra ?? {})) query.set(k, v);
    const qs = query.toString();
    return qs ? `?${qs}` : "";
  };

  let page: LogPage;
  try {
    const query = new URLSearchParams(filters);
    if (params.before) query.set("before", params.before);
    page = await getLogs(query);
  } catch (err) {
    return (
      <>
        <PageHeader title="Logs" />
        <ApiProblem error={err} />
      </>
    );
  }

  const today = new Intl.DateTimeFormat("en-CA", { timeZone: "Asia/Kolkata" }).format(new Date());
  const reset = ["before"];

  return (
    <>
      <PageHeader
        title="Logs"
        description="Every action in Acadex, newest first. The log is hash-chained, so any edit is detectable."
        actions={
          <>
            <UrlInput param="who" value={params.who ?? ""} label="Who" type="search" placeholder="Name" reset={reset} className="w-36" />
            <UrlSelect param="role" value={params.role ?? "all"} options={ROLES} label="Role" reset={reset} />
            <UrlSelect param="category" value={params.category ?? "all"} options={CATEGORIES} label="Category" reset={reset} />
            <UrlSelect param="outcome" value={params.outcome ?? "all"} options={OUTCOMES} label="Outcome" reset={reset} className="w-36" />
            <UrlInput param="from" value={params.from ?? ""} label="From" type="date" max={params.to || today} reset={reset} className="w-36" />
            <span className="text-muted-foreground text-sm">to</span>
            <UrlInput param="to" value={params.to ?? ""} label="To" type="date" min={params.from} max={today} reset={reset} className="w-36" />
            <Button variant="outline" asChild>
              {/* A plain link: a route handler streams the file. */}
              <a href={`/logs/export${withFilters()}`} download>
                <Download />
                Export CSV
              </a>
            </Button>
          </>
        }
      />
      <Card>
        <CardContent>
          {page.entries.length === 0 ? (
            <p className="text-muted-foreground">{filters.size > 0 ? "Nothing matches these filters." : "Nothing logged yet."}</p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Time</TableHead>
                  <TableHead>Who</TableHead>
                  <TableHead>Action</TableHead>
                  <TableHead>Outcome</TableHead>
                  <TableHead>IP</TableHead>
                  <TableHead>Device</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {page.entries.map((entry) => (
                  <TableRow key={entry.id} className="align-top">
                    <TableCell className="whitespace-nowrap tabular-nums">
                      {TIME.format(new Date(entry.occurred_at))}
                    </TableCell>
                    <TableCell>{entry.actor_name ?? SYSTEM_ACTORS[entry.actor_role ?? ""] ?? "—"}</TableCell>
                    <TableCell className="max-w-md min-w-56 whitespace-normal">
                      <details>
                        <summary className="cursor-pointer">{ACTIONS[entry.action] ?? entry.action}</summary>
                        <dl className="text-muted-foreground mt-1 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 font-mono text-xs break-words">
                          <dt>action</dt>
                          <dd>{entry.action}</dd>
                          <dt>resource</dt>
                          <dd>
                            {entry.resource_type}
                            {entry.resource_id && ` ${entry.resource_id}`}
                          </dd>
                          {Object.entries(entry.metadata).map(([key, value]) => (
                            <MetaRow key={key} label={key} value={value} />
                          ))}
                          {entry.trace_id && (
                            <>
                              <dt>trace</dt>
                              <dd>{entry.trace_id}</dd>
                            </>
                          )}
                        </dl>
                      </details>
                    </TableCell>
                    <TableCell>
                      <OutcomeBadge outcome={entry.outcome} />
                    </TableCell>
                    <TableCell className="font-mono text-xs">{entry.ip ?? "—"}</TableCell>
                    <TableCell className="text-sm">{describeDevice(entry.user_agent)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
          <div className="mt-4 flex justify-between text-sm">
            {params.before ? (
              <Link href={`/logs${withFilters()}`} className="underline-offset-4 hover:underline">
                Newest
              </Link>
            ) : (
              <span />
            )}
            {page.next_before && (
              <Link href={`/logs${withFilters({ before: String(page.next_before) })}`} className="underline-offset-4 hover:underline">
                Older
              </Link>
            )}
          </div>
        </CardContent>
      </Card>
    </>
  );
}

function MetaRow({ label, value }: { label: string; value: unknown }) {
  return (
    <>
      <dt>{label}</dt>
      <dd>{typeof value === "string" ? value : JSON.stringify(value)}</dd>
    </>
  );
}

function OutcomeBadge({ outcome }: { outcome: LogEntry["outcome"] }) {
  if (outcome === "success") return <Badge variant="outline">Success</Badge>;
  return <Badge variant="destructive">{outcome === "denied" ? "Denied" : "Error"}</Badge>;
}

// First match wins: Edge's user agent also says Chrome, and Chrome's says Safari.
const BROWSERS: [RegExp, string][] = [
  [/Edg\//, "Edge"],
  [/Firefox\//, "Firefox"],
  [/Chrome\//, "Chrome"],
  [/Safari\//, "Safari"],
];
const SYSTEMS: [RegExp, string][] = [
  [/Android/, "Android"],
  [/iPhone|iPad/, "iOS"],
  [/Windows/, "Windows"],
  [/Mac OS X/, "macOS"],
  [/Linux/, "Linux"],
];

/** "Chrome on Linux" from a user-agent string; good enough for a log, not for analytics. */
function describeDevice(userAgent: string | null): string {
  if (!userAgent) return "—";
  const browser = BROWSERS.find(([pattern]) => pattern.test(userAgent))?.[1];
  const os = SYSTEMS.find(([pattern]) => pattern.test(userAgent))?.[1];
  if (browser && os) return `${browser} on ${os}`;
  return browser ?? os ?? userAgent.slice(0, 40);
}
