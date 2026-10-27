import { ChevronLeft, ChevronRight, Download } from "lucide-react";
import Link from "next/link";

import { ApiProblem } from "@/components/api-problem";
import { EVENT_KIND, EventChip } from "@/components/event-kind";
import { PageHeader } from "@/components/page-header";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { asUser, getCalendar, getMe } from "@/lib/acadex";
import type { CalendarEvent, EventKind } from "@/lib/api";
import { cn } from "@/lib/utils";

import { EventDialog, RemoveEventButton } from "./manage-events";

export const dynamic = "force-dynamic";

const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTH = new Intl.DateTimeFormat("en-IN", { month: "long", year: "numeric", timeZone: "UTC" });
const DAY = new Intl.DateTimeFormat("en-IN", { weekday: "short", day: "numeric", month: "short", timeZone: "UTC" });

/** Dates are handled as UTC midnights so a day never shifts with the time zone. */
const iso = (d: Date) => d.toISOString().slice(0, 10);
const utc = (y: number, m: number, d: number) => new Date(Date.UTC(y, m, d));

export default async function CalendarPage({ searchParams }: { searchParams: Promise<{ month?: string; view?: string }> }) {
  const params = await searchParams;
  const view = params.view === "agenda" ? "agenda" : "month";
  const today = new Date();
  const [year, month] = /^\d{4}-\d{2}$/.test(params.month ?? "")
    ? params.month!.split("-").map(Number)
    : [today.getUTCFullYear(), today.getUTCMonth() + 1];
  const first = utc(year, month - 1, 1);
  const last = utc(year, month, 0);
  // The grid runs Monday to Sunday, so it starts up to six days before the 1st.
  const gridStart = utc(year, month - 1, 1 - ((first.getUTCDay() + 6) % 7));
  const gridEnd = utc(last.getUTCFullYear(), last.getUTCMonth(), last.getUTCDate() + (6 - ((last.getUTCDay() + 6) % 7)));
  const agendaEnd = utc(year, month + 2, 0);

  const me = await getMe();
  const canManage = me.permissions.includes("calendar:manage");
  let events: CalendarEvent[];
  let programs: { id: number; name: string }[] = [];
  try {
    [events, programs] = await Promise.all([
      view === "month" ? getCalendar(iso(gridStart), iso(gridEnd)) : getCalendar(iso(first), iso(agendaEnd)),
      canManage ? asUser<{ id: number; name: string }[]>("/programs") : Promise.resolve([]),
    ]);
  } catch (err) {
    return (
      <>
        <PageHeader title="Academic calendar" />
        <ApiProblem error={err} />
      </>
    );
  }

  const shift = (months: number) => {
    const d = utc(year, month - 1 + months, 1);
    return `/academic/calendar?month=${iso(d).slice(0, 7)}&view=${view}`;
  };

  return (
    <>
      <PageHeader title="Academic calendar" description="Holidays, exams, deadlines and events."
        actions={
          <>
            <div className="flex gap-2">
              <Button variant="outline" asChild>
                {/* A file download from a route handler, not a page. */}
                <a href="/academic/calendar.ics" download>
                  <Download />
                  Export (.ics)
                </a>
              </Button>
              {canManage && <EventDialog programs={programs} />}
            </div>
          </>
        }
      />

      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <Button variant="outline" size="icon" asChild>
            <Link href={shift(-1)} aria-label="Previous month">
              <ChevronLeft />
            </Link>
          </Button>
          <h2 className="min-w-44 text-center text-lg font-semibold">{MONTH.format(first)}</h2>
          <Button variant="outline" size="icon" asChild>
            <Link href={shift(1)} aria-label="Next month">
              <ChevronRight />
            </Link>
          </Button>
        </div>
        <nav aria-label="View" className="flex gap-1 rounded-md border p-0.5">
          {(["month", "agenda"] as const).map((v) => (
            <Link
              key={v}
              href={`/academic/calendar?month=${iso(first).slice(0, 7)}&view=${v}`}
              aria-current={v === view ? "page" : undefined}
              className={cn("rounded px-3 py-1 text-sm capitalize", v === view ? "bg-primary text-primary-foreground" : "hover:bg-muted")}
            >
              {v}
            </Link>
          ))}
        </nav>
      </div>

      <ul className="text-muted-foreground mb-4 flex flex-wrap gap-3 text-xs" aria-label="Legend">
        {(Object.keys(EVENT_KIND) as EventKind[]).map((k) => {
          const { icon: Icon, label } = EVENT_KIND[k];
          return (
            <li key={k} className="flex items-center gap-1">
              <Icon className="size-3.5" aria-hidden />
              {label}
            </li>
          );
        })}
      </ul>

      {view === "month" ? (
        <MonthGrid events={events} gridStart={gridStart} gridEnd={gridEnd} month={month - 1} today={iso(today)} />
      ) : (
        <Agenda events={events} canManage={canManage} programs={programs} />
      )}
    </>
  );
}

function MonthGrid({ events, gridStart, gridEnd, month, today }: { events: CalendarEvent[]; gridStart: Date; gridEnd: Date; month: number; today: string }) {
  const days: Date[] = [];
  for (let d = gridStart; d <= gridEnd; d = utc(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate() + 1)) days.push(d);
  const weeks = Array.from({ length: days.length / 7 }, (_, i) => days.slice(i * 7, i * 7 + 7));
  const on = (day: string) => events.filter((e) => e.starts_on <= day && e.ends_on >= day);

  return (
    <Card>
      <CardContent className="overflow-x-auto p-2 sm:p-4">
        <table className="w-full min-w-[42rem] table-fixed border-collapse">
          <thead>
            <tr>
              {WEEKDAYS.map((d) => (
                <th key={d} scope="col" className="text-muted-foreground pb-2 text-xs font-medium">
                  {d}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {weeks.map((week) => (
              <tr key={iso(week[0])}>
                {week.map((day) => {
                  const key = iso(day);
                  const dayEvents = on(key);
                  const outside = day.getUTCMonth() !== month;
                  return (
                    <td key={key} className={cn("h-24 border p-1 align-top", outside && "bg-muted/40")}>
                      <div
                        className={cn(
                          "mb-1 text-xs tabular-nums",
                          outside && "text-muted-foreground",
                          key === today && "bg-primary text-primary-foreground inline-grid size-5 place-items-center rounded-full",
                        )}
                        aria-label={DAY.format(day)}
                      >
                        {day.getUTCDate()}
                      </div>
                      <div className="grid gap-0.5">
                        {dayEvents.slice(0, 3).map((e) => (
                          <EventChip key={e.id} kind={e.kind} title={e.title} />
                        ))}
                        {dayEvents.length > 3 && <span className="text-muted-foreground text-xs">+{dayEvents.length - 3} more</span>}
                      </div>
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </CardContent>
    </Card>
  );
}

function Agenda({
  events,
  canManage,
  programs,
}: {
  events: CalendarEvent[];
  canManage: boolean;
  programs: { id: number; name: string }[];
}) {
  if (events.length === 0) return <p className="text-muted-foreground">Nothing scheduled in these three months.</p>;
  const range = (e: CalendarEvent) => {
    const start = DAY.format(new Date(`${e.starts_on}T00:00:00Z`));
    return e.starts_on === e.ends_on ? start : `${start} – ${DAY.format(new Date(`${e.ends_on}T00:00:00Z`))}`;
  };
  return (
    <Card>
      <CardContent className="divide-y p-0">
        {events.map((e) => {
          const { icon: Icon, label, chip } = EVENT_KIND[e.kind];
          return (
            <div key={e.id} className="flex items-start gap-3 p-4">
              <span className={cn("grid size-9 shrink-0 place-items-center rounded-md border", chip)}>
                <Icon className="size-4" aria-hidden />
              </span>
              <div className="min-w-0 flex-1">
                <div className="font-medium">{e.title}</div>
                <div className="text-muted-foreground text-sm">
                  {range(e)} · {label}
                  {e.program_name && ` · ${e.program_name}`}
                </div>
                {e.description && <p className="text-muted-foreground mt-1 text-sm">{e.description}</p>}
              </div>
              {canManage && (
                <div className="flex shrink-0 gap-1">
                  <EventDialog programs={programs} event={e} />
                  <RemoveEventButton id={e.id} title={e.title} />
                </div>
              )}
            </div>
          );
        })}
      </CardContent>
    </Card>
  );
}
