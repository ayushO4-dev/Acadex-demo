import Link from "next/link";

import { ApiProblem } from "@/components/api-problem";
import { AttendanceBar } from "@/components/attendance-bar";
import { AttendanceMonth } from "@/components/attendance-month";
import { AttendanceRing } from "@/components/attendance-ring";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { UrlSelect } from "@/components/url-select";
import { getAttendance, getAttendanceMonth } from "@/lib/acadex";
import type { AttendanceMonth as AttendanceMonthData, AttendanceReport } from "@/lib/api";
import { cn } from "@/lib/utils";

/**
 * A student's attendance: overall ring, a colour-coded month, and per-subject
 * bars that open the session list. Used by the student and by staff, with
 * `basePath` deciding where links go.
 */
export async function StudentAttendance({
  studentId,
  semester,
  month,
  basePath,
  sessionHref,
}: {
  studentId: string;
  semester?: number;
  month?: string;
  /** This page's path, for month and year links. */
  basePath: string;
  /** Where a subject's sessions are listed. */
  sessionHref: (code: string, academicYear: number) => string;
}) {
  let report: AttendanceReport, calendar: AttendanceMonthData;
  try {
    report = await getAttendance(studentId, semester);
    // A past semester opens on the month its attendance ends; the current one, on today.
    const opening =
      month ?? (report.semester !== report.student.current_semester ? report.last_session_on?.slice(0, 7) : undefined);
    calendar = await getAttendanceMonth(studentId, opening);
  } catch (err) {
    return <ApiProblem error={err} />;
  }
  const link = (params: Record<string, string | number>) => {
    const q = new URLSearchParams({ ...(semester ? { semester: String(semester) } : {}), ...(month ? { month } : {}) });
    for (const [k, v] of Object.entries(params)) q.set(k, String(v));
    return `${basePath}?${q}`;
  };
  const { overall } = report;
  const inShortage = report.subjects.filter((s) => s.shortage).length;

  return (
    <div className="grid gap-4">
      <div className="grid gap-4 lg:grid-cols-2">
        <Card className="flex flex-col">
          <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-3">
            <div className="grid gap-1.5">
              <CardTitle>Overall</CardTitle>
              <CardDescription>Below {report.shortage_threshold_pct}% a subject is in shortage.</CardDescription>
            </div>
            <UrlSelect
              param="semester"
              value={String(report.semester ?? report.student.current_semester)}
              label="Semester"
              reset={["month", "year"]}
              options={report.semesters.map((s) => ({
                value: String(s),
                label: `Semester ${s}${s === report.student.current_semester ? " (current)" : ""}`,
              }))}
            />
          </CardHeader>
          {/* Laid out by the card's own width, not the window's: side by side only when there's room. */}
          {/* The calendar's content sets the row's height; this card matches it, its content centred. */}
          <CardContent className="@container flex flex-1 items-center">
            <div className="flex w-full flex-col items-center gap-4 @md:flex-row @md:justify-around">
            <AttendanceRing percentage={overall.percentage} threshold={report.shortage_threshold_pct} shortage={overall.shortage} size={132} />
            <div className="grid w-full max-w-60 min-w-0 gap-4">
              <dl className="grid gap-2">
                <Figure label="Attended" value={overall.attended} />
                <Figure label="Not attended" value={overall.conducted - overall.attended} />
                <Figure label="In shortage" value={`${inShortage} / ${report.subjects.length}`} danger={inShortage > 0} />
              </dl>
            </div>
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardContent>
            <AttendanceMonth data={calendar} hrefFor={(m) => link({ month: m })} />
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>By subject</CardTitle>
          <CardDescription>
            The line on each bar marks the threshold. Theory and practicals are each held to it. Open a subject for its sessions.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {report.subjects.length === 0 ? (
            <p className="text-muted-foreground">No sessions recorded this academic year.</p>
          ) : (
            <ul className="divide-y">
              {report.subjects.map((subject) => (
                <li key={subject.code}>
                  <Link
                    href={sessionHref(subject.code, report.academic_year)}
                    className="hover:bg-muted/50 -mx-2 grid gap-2 rounded-md px-2 py-3 sm:grid-cols-[1fr_16rem_auto] sm:items-center sm:gap-4"
                  >
                    <div>
                      <span className="font-medium">{subject.code}</span>{" "}
                      <span className="text-muted-foreground">{subject.name}</span>
                      <div className="text-muted-foreground text-xs tabular-nums">
                        {subject.attended}/{subject.conducted} sessions · {subject.credits} credits
                      </div>
                    </div>
                    <div className="grid gap-1.5">
                      {(
                        [
                          ["Theory", subject.theory],
                          ["Practical", subject.practical],
                        ] as const
                      )
                        .filter(([, part]) => part)
                        .map(([label, part]) => (
                          <div key={label} className="grid grid-cols-[4.5rem_1fr_3.5rem] items-center gap-2 text-xs">
                            <span className="text-muted-foreground">{label}</span>
                            <AttendanceBar percentage={part!.percentage} threshold={report.shortage_threshold_pct} shortage={part!.shortage} />
                            <span className={cn("text-right tabular-nums", part!.shortage && "text-destructive font-medium")}>
                              {part!.percentage.toFixed(1)}%
                            </span>
                          </div>
                        ))}
                    </div>
                    <div className="flex items-center gap-2 sm:justify-end">
                      <ShortageBadge shortage={subject.shortage} />
                    </div>
                  </Link>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

// Text as well as colour, so status never depends on colour alone.
export function ShortageBadge({ shortage }: { shortage: boolean }) {
  return shortage ? <Badge variant="destructive">Shortage</Badge> : <Badge variant="outline">OK</Badge>;
}


function Figure({ label, value, danger = false }: { label: string; value: number | string; danger?: boolean }) {
  return (
    <div className="flex items-baseline justify-between gap-3 rounded-md border px-3 py-2">
      <dt className="text-muted-foreground truncate text-sm" title={label}>
        {label}
      </dt>
      <dd className={cn("text-xl font-semibold tabular-nums", danger && "text-destructive")}>{value}</dd>
    </div>
  );
}
