import { ChevronLeft, ChevronRight } from "lucide-react";
import Link from "next/link";

import { Button } from "@/components/ui/button";
import type { AttendanceMonth as Month } from "@/lib/api";
import { cn } from "@/lib/utils";

const WEEKDAYS = ["M", "T", "W", "T", "F", "S", "S"];
const TITLE = new Intl.DateTimeFormat("en-IN", { month: "long", year: "numeric", timeZone: "UTC" });

/** Days coloured by how much of them was attended; the count is in each cell's label too.
 * Rows are a fixed height, so the calendar is as tall at any width. */
export function AttendanceMonth({ data, hrefFor }: { data: Month; hrefFor: (month: string) => string }) {
  const [y, m] = data.month.split("-").map(Number);
  const first = new Date(Date.UTC(y, m - 1, 1));
  const daysIn = new Date(Date.UTC(y, m, 0)).getUTCDate();
  const lead = (first.getUTCDay() + 6) % 7;
  const byDate = new Map(data.days.map((d) => [d.date, d]));
  const shift = (n: number) => new Date(Date.UTC(y, m - 1 + n, 1)).toISOString().slice(0, 7);

  return (
    <div>
      <div className="mb-2 flex items-center justify-between">
        <Button variant="ghost" size="icon" asChild>
          <Link href={hrefFor(shift(-1))} aria-label="Previous month" scroll={false}>
            <ChevronLeft />
          </Link>
        </Button>
        <span className="text-sm font-medium">{TITLE.format(first)}</span>
        <Button variant="ghost" size="icon" asChild>
          <Link href={hrefFor(shift(1))} aria-label="Next month" scroll={false}>
            <ChevronRight />
          </Link>
        </Button>
      </div>
      <div className="grid grid-cols-7 gap-1 text-center text-xs" role="grid">
        {WEEKDAYS.map((d, i) => (
          <div key={i} className="text-muted-foreground pb-1" aria-hidden>
            {d}
          </div>
        ))}
        {Array.from({ length: lead }, (_, i) => (
          <div key={`lead-${i}`} />
        ))}
        {Array.from({ length: daysIn }, (_, i) => {
          const date = `${data.month}-${String(i + 1).padStart(2, "0")}`;
          const day = byDate.get(date);
          const attended = day ? day.present + day.late : 0;
          const tone = !day
            ? "text-muted-foreground"
            : attended === day.sessions
              ? "bg-emerald-500/20 text-emerald-900 dark:text-emerald-200"
              : attended === 0
                ? "bg-destructive/20 text-destructive"
                : "bg-amber-500/25 text-amber-900 dark:text-amber-200";
          const label = day
            ? `${date}: ${attended} of ${day.sessions} sessions attended${day.excused ? `, ${day.excused} excused` : ""}`
            : `${date}: no sessions`;
          return (
            <div key={date} role="gridcell" aria-label={label} title={label} className={cn("grid h-8 place-items-center rounded tabular-nums", tone)}>
              {i + 1}
            </div>
          );
        })}
      </div>
      <ul className="text-muted-foreground mt-3 flex flex-wrap gap-3 text-xs">
        <li className="flex items-center gap-1"><span className="size-3 rounded bg-emerald-500/40" aria-hidden />All attended</li>
        <li className="flex items-center gap-1"><span className="size-3 rounded bg-amber-500/50" aria-hidden />Some missed</li>
        <li className="flex items-center gap-1"><span className="bg-destructive/40 size-3 rounded" aria-hidden />All missed</li>
      </ul>
    </div>
  );
}
