import type { AttendanceStatusName, Session } from "@/lib/api";
import { cn } from "@/lib/utils";

export const MONTH = new Intl.DateTimeFormat("en-IN", { month: "long", year: "numeric", timeZone: "UTC" });
const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
export const STATUS: Record<AttendanceStatusName, { label: string; variant: "outline" | "secondary" | "destructive"; chip: string }> = {
  present: { label: "Present", variant: "outline", chip: "border-emerald-500/40 bg-emerald-500/15 text-emerald-900 dark:text-emerald-200" },
  late: { label: "Late", variant: "secondary", chip: "border-amber-500/40 bg-amber-500/15 text-amber-900 dark:text-amber-200" },
  excused: { label: "Excused", variant: "secondary", chip: "border-sky-500/40 bg-sky-500/15 text-sky-900 dark:text-sky-200" },
  absent: { label: "Absent", variant: "destructive", chip: "border-red-500/40 bg-red-500/15 text-red-900 dark:text-red-200" },
};
export const when = (s: Session) => (s.starts_at ? `${s.starts_at}–${s.ends_at}` : `Period ${s.slot}`);
export const kindLabel = (s: Session) => (s.session_kind === "practical" ? "Practical" : "Theory");

/** A month, Monday first, each day listing its sessions with their times. */
export function MonthGrid({ month, byDate }: { month: string; byDate: Map<string, Session[]> }) {
  const [y, m] = month.split("-").map(Number);
  const first = new Date(Date.UTC(y, m - 1, 1));
  const days = new Date(Date.UTC(y, m, 0)).getUTCDate();
  const lead = (first.getUTCDay() + 6) % 7;
  const today = new Date().toISOString().slice(0, 10);
  return (
    <div className="grid grid-cols-7 gap-1" role="grid" aria-label={MONTH.format(first)}>
      {WEEKDAYS.map((d) => (
        <div key={d} className="text-muted-foreground pb-1 text-center text-xs" aria-hidden>
          {d}
        </div>
      ))}
      {Array.from({ length: lead }, (_, i) => (
        <div key={`lead-${i}`} />
      ))}
      {Array.from({ length: days }, (_, i) => {
        const date = `${month}-${String(i + 1).padStart(2, "0")}`;
        const list = byDate.get(date) ?? [];
        return (
          <div
            key={date}
            role="gridcell"
            aria-label={list.length ? `${date}: ${list.map((s) => `${when(s)} ${kindLabel(s)} ${STATUS[s.status].label}`).join(", ")}` : `${date}: no session`}
            className={cn("min-h-16 rounded-md border p-1 sm:min-h-20", list.length === 0 && "bg-muted/30 border-transparent", date === today && "ring-primary ring-1")}
          >
            <div className={cn("text-xs tabular-nums", list.length === 0 && "text-muted-foreground")}>{i + 1}</div>
            <div className="mt-1 grid gap-1">
              {list.map((s) => (
                <div key={s.slot} className={cn("rounded border px-1 py-0.5 text-[10px] leading-tight tabular-nums sm:text-xs", STATUS[s.status].chip)} title={`${when(s)} · ${kindLabel(s)} · ${STATUS[s.status].label}`}>
                  {s.session_kind === "practical" && (
                    <span className="mr-0.5 rounded-sm bg-current/15 px-0.5 font-semibold" aria-hidden>
                      Pr
                    </span>
                  )}
                  <span className="font-medium">{s.starts_at ?? `P${s.slot}`}</span>
                  <span className="hidden lg:inline">{s.ends_at ? `–${s.ends_at}` : ""}</span>
                  <span className="hidden sm:block">{STATUS[s.status].label}</span>
                </div>
              ))}
            </div>
          </div>
        );
      })}
    </div>
  );
}
