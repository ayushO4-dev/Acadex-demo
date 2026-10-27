import Link from "next/link";

import { ApiProblem } from "@/components/api-problem";
import { PageHeader } from "@/components/page-header";
import { StudentAttendance } from "@/components/student-attendance";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { asUser, getMe, getOfferings, getRoster } from "@/lib/acadex";
import type { Offering, Roster, SessionKind, Week } from "@/lib/api";
import { cn } from "@/lib/utils";

import { CorrectionsList } from "./corrections-list";

import { MarkingForm } from "./marking-form";
import { SessionPicker } from "./session-picker";

export const dynamic = "force-dynamic";

type Search = { offering?: string; date?: string; slot?: string; kind?: string; semester?: string; month?: string };

export default async function AttendancePage({ searchParams }: { searchParams: Promise<Search> }) {
  const me = await getMe();
  const search = await searchParams;

  if (me.role === "faculty") {
    return (
      <div className="grid gap-6">
        <MarkAttendance search={search} />
        <CorrectionsList
          canDecide={false}
          title="Correction requests"
          description="Students' requests on your subjects. Their HOD decides; you see the outcome."
        />
      </div>
    );
  }
  if (me.student) {
    return (
      <>
        <PageHeader title="Attendance" description={`${me.student.roll_no} · ${me.student.program_name}`} />
        <div className="grid gap-4">
          <StudentAttendance
            studentId={me.student.id}
            semester={Number(search.semester) || undefined}
            month={search.month}
            basePath="/academic/attendance"
            sessionHref={(code, year) => `/academic/attendance/${encodeURIComponent(code)}?year=${year}`}
          />
          <CorrectionsList
            canDecide={false}
            title="My correction requests"
            description="Open a subject and choose a session to request a correction. Your HOD decides."
          />
        </div>
      </>
    );
  }
  const canDecide = me.permissions.includes("corrections:decide");
  return (
    <>
      <PageHeader
        title="Attendance"
        description={canDecide ? `Corrections for ${me.department?.name ?? "your department"}` : undefined}
      />
      <div className="grid gap-4">
        {canDecide && (
          <CorrectionsList
            canDecide
            title="Correction requests"
            description="Approving rewrites the session's mark, recorded as yours, and the student's summary updates at once."
          />
        )}
      </div>
    </>
  );
}

async function MarkAttendance({ search }: { search: Search }) {
  let offerings: Offering[];
  try {
    offerings = await getOfferings();
  } catch (err) {
    return (
      <>
        <PageHeader title="Mark attendance" />
        <ApiProblem error={err} />
      </>
    );
  }
  if (offerings.length === 0) {
    return (
      <>
        <PageHeader title="Mark attendance" />
        <p className="text-muted-foreground">You have no offerings this academic year.</p>
      </>
    );
  }

  const now = new Date();
  const today = new Intl.DateTimeFormat("en-CA", { timeZone: "Asia/Kolkata" }).format(now);
  const date = search.date && search.date <= today ? search.date : today;
  // The teacher's timetable for that day: one click picks the class.
  const week = await asUser<Week>("/academic/timetable").catch(() => null);
  const weekday = new Date(`${date}T00:00:00Z`).getUTCDay();
  const classes = (week?.view === "teacher" ? week.entries : []).filter((e) => e.weekday === weekday);
  const period = (slot: number) => week?.periods.find((p) => p.slot === slot);
  // Arriving without a choice: the class going on now (or the last one today).
  const clock = new Intl.DateTimeFormat("en-GB", { timeZone: "Asia/Kolkata", hour: "2-digit", minute: "2-digit" }).format(now);
  const current = search.offering
    ? undefined
    : [...classes].reverse().find((e) => (period(e.slot)?.starts ?? "99:99") <= clock) ?? classes[0];
  const wanted = search.offering ?? current?.offering_id;
  const offeringId = offerings.some((o) => o.id === wanted) ? wanted! : offerings[0].id;
  const slot = Number(search.slot) >= 1 && Number(search.slot) <= 8 ? Number(search.slot) : (current?.slot ?? 1);

  let roster: Roster | null = null;
  let error: unknown = null;
  try {
    roster = await getRoster(offeringId, date, slot);
  } catch (err) {
    error = err;
  }

  // Labs are all practical; subjects with practicals choose; a marked session keeps its kind.
  const offering = offerings.find((o) => o.id === offeringId)!;
  const kind: SessionKind =
    offering.kind === "lab"
      ? "practical"
      : (roster?.session_kind ??
        (offering.has_practical && (search.kind ?? current?.session_kind) === "practical" ? "practical" : "theory"));

  return (
    <>
      <PageHeader title="Mark attendance" description="Choose the session, mark the absentees, save." />
      {classes.length > 0 && (
        <nav aria-label="Your classes that day" className="mb-3 flex flex-wrap gap-2">
          {classes.map((e) => {
            const selected = e.offering_id === offeringId && e.slot === slot;
            return (
              <Link
                key={e.id}
                href={`/academic/attendance?${new URLSearchParams({ offering: e.offering_id, date, slot: String(e.slot), kind: e.session_kind })}`}
                className={cn(
                  "rounded-md border px-3 py-1.5 text-sm",
                  selected ? "border-primary bg-primary text-primary-foreground" : "hover:bg-muted",
                )}
              >
                <span className="font-medium">P{e.slot}</span> {period(e.slot)?.starts} · {e.code}
                {e.session_kind === "practical" ? " practical" : ""}
                {e.room ? ` · ${e.room}` : ""}
              </Link>
            );
          })}
        </nav>
      )}
      <Card className="mb-4">
        <CardContent>
          <SessionPicker offerings={offerings} offeringId={offeringId} date={date} slot={slot} kind={kind} today={today} />
        </CardContent>
      </Card>
      {roster ? (
        <Card>
          <CardHeader>
            <CardTitle>
              {roster.offering.code} · {roster.offering.name}
            </CardTitle>
            <CardDescription>
              {roster.offering.program_name} · Semester {roster.offering.semester} · {roster.students.length} students ·{" "}
              {kind === "practical" ? "practical" : "theory"} session
            </CardDescription>
          </CardHeader>
          <CardContent>
            {/* Keyed by session, so changing subject, date or period starts a fresh form. */}
            <MarkingForm key={`${offeringId}-${date}-${slot}-${kind}`} roster={roster} sessionKind={kind} />
          </CardContent>
        </Card>
      ) : (
        <ApiProblem error={error} />
      )}
    </>
  );
}
