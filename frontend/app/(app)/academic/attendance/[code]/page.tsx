import { ArrowLeft, ChevronLeft, ChevronRight } from "lucide-react";
import Link from "next/link";

import { ApiProblem } from "@/components/api-problem";
import { PageHeader } from "@/components/page-header";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { getMe, getSessions, getStudent } from "@/lib/acadex";
import type { Session, Student } from "@/lib/api";
import { academicYear, cn } from "@/lib/utils";

import { MONTH, MonthGrid, STATUS, when } from "@/components/session-calendar";

import { RequestCorrection } from "../correction-dialogs";

export const dynamic = "force-dynamic";

const DATE = new Intl.DateTimeFormat("en-IN", { weekday: "short", day: "numeric", month: "short", timeZone: "UTC" });

/** Every session of one subject, for a student (theirs) or staff (?student=). */
export default async function SessionsPage({
  params,
  searchParams,
}: {
  params: Promise<{ code: string }>;
  searchParams: Promise<{ year?: string; student?: string; month?: string }>;
}) {
  const { code } = await params;
  const { year, student: studentParam, month: wantedMonth } = await searchParams;
  const me = await getMe();
  const own = !studentParam;

  let student: Student;
  let sessions: Session[];
  try {
    student = own && me.student ? me.student : await getStudent(studentParam ?? "");
    sessions = await getSessions(student.id, decodeURIComponent(code), Number(year) || undefined);
  } catch (err) {
    return (
      <>
        <PageHeader title={decodeURIComponent(code)} />
        <ApiProblem error={err} />
      </>
    );
  }
  const back = "/academic/attendance";
  const missed = sessions.filter((s) => s.status === "absent").length;

  // The months with sessions, oldest first; open on the one asked for, else the latest.
  const months = [...new Set(sessions.map((s) => s.date.slice(0, 7)))].sort();
  const month = months.includes(wantedMonth ?? "") ? (wantedMonth as string) : months[months.length - 1];
  const at = months.indexOf(month);
  const monthLink = (m: string) =>
    `?${new URLSearchParams({ ...(year ? { year } : {}), ...(studentParam ? { student: studentParam } : {}), month: m })}`;
  const inMonth = sessions
    .filter((s) => s.date.startsWith(month ?? "-"))
    .sort((a, b) => a.date.localeCompare(b.date) || a.slot - b.slot);
  const byDate = new Map<string, Session[]>();
  for (const s of inMonth) byDate.set(s.date, [...(byDate.get(s.date) ?? []), s]);

  return (
    <>
      <Link href={back} className="text-muted-foreground mb-2 inline-flex items-center gap-1 text-sm hover:underline">
        <ArrowLeft className="size-4" aria-hidden />
        Attendance
      </Link>
      <PageHeader
        title={`${decodeURIComponent(code)} sessions`}
        description={`${own ? "" : `${student.display_name} · `}${year ? `Academic year ${academicYear(Number(year))} · ` : ""}${sessions.length} sessions, ${missed} absent`}
      />
      {sessions.length === 0 || !month ? (
        <p className="text-muted-foreground">No sessions recorded.</p>
      ) : (
        <div className="grid gap-4">
          <Card>
            <CardHeader className="flex flex-row items-center justify-between gap-2">
              <CardTitle>{MONTH.format(new Date(`${month}-01T00:00:00Z`))}</CardTitle>
              <div className="flex items-center gap-1">
                <Button variant="ghost" size="icon" asChild>
                  {at > 0 ? (
                    <Link href={monthLink(months[at - 1])} aria-label="Previous month" scroll={false}>
                      <ChevronLeft />
                    </Link>
                  ) : (
                    <span aria-hidden className="opacity-40">
                      <ChevronLeft />
                    </span>
                  )}
                </Button>
                <Button variant="ghost" size="icon" asChild>
                  {at < months.length - 1 ? (
                    <Link href={monthLink(months[at + 1])} aria-label="Next month" scroll={false}>
                      <ChevronRight />
                    </Link>
                  ) : (
                    <span aria-hidden className="opacity-40">
                      <ChevronRight />
                    </span>
                  )}
                </Button>
              </div>
            </CardHeader>
            <CardContent>
              <MonthGrid month={month} byDate={byDate} />
              <div className="text-muted-foreground mt-3 flex flex-wrap gap-3 text-xs">
                {Object.entries(STATUS).map(([key, s]) => (
                  <span key={key} className="flex items-center gap-1">
                    <span className={cn("size-3 rounded border", s.chip)} aria-hidden />
                    {s.label}
                  </span>
                ))}
                {sessions.some((s) => s.session_kind === "practical") && (
                  <span className="flex items-center gap-1">
                    <span className="bg-muted rounded-sm px-0.5 font-semibold">Pr</span> practical
                  </span>
                )}
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Sessions this month</CardTitle>
              <CardDescription>
                {inMonth.length} session{inMonth.length === 1 ? "" : "s"}, {inMonth.filter((s) => s.status === "absent").length} absent
              </CardDescription>
            </CardHeader>
            <CardContent>
              <ul className="divide-y">
                {[...inMonth].reverse().map((s) => (
                  <li key={`${s.date}-${s.slot}`} className="flex flex-wrap items-center justify-between gap-3 py-2.5 text-sm">
                    <div className="flex items-center gap-3">
                      <div className="w-28 tabular-nums">
                        <div className="font-medium">{DATE.format(new Date(`${s.date}T00:00:00Z`))}</div>
                        <div className="text-muted-foreground text-xs">
                          {when(s)} · P{s.slot}
                          {s.session_kind === "practical" && " · Practical"}
                        </div>
                      </div>
                      <Badge variant={STATUS[s.status].variant}>{STATUS[s.status].label}</Badge>
                      <span className="text-muted-foreground hidden sm:inline">{s.marked_by ?? ""}</span>
                    </div>
                    <div>
                      {s.correction === "pending" ? (
                        <Badge variant="secondary">Correction pending</Badge>
                      ) : s.correction ? (
                        <span className="text-muted-foreground text-xs">Correction {s.correction}</span>
                      ) : (
                        own && me.permissions.includes("corrections:request") && s.status !== "present" && <RequestCorrection session={s} />
                      )}
                    </div>
                  </li>
                ))}
              </ul>
            </CardContent>
          </Card>
        </div>
      )}
    </>
  );
}
