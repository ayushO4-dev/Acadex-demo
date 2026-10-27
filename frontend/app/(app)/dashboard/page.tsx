import Link from "next/link";

import { ApiProblem } from "@/components/api-problem";
import { NoticeCard } from "@/components/notice-card";
import { PageHeader } from "@/components/page-header";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { getAttendance, getMe, getNotices, getPayable } from "@/lib/acadex";
import type { Department } from "@/lib/api";
import { ROLE_LABELS } from "@/lib/roles";
import { formatPaise } from "@/lib/utils";
import { navFor } from "@/lib/nav";

export const dynamic = "force-dynamic";

export default async function Dashboard() {
  const me = await getMe();
  const built = navFor(me.permissions)
    .flatMap((section) => section.items)
    .filter((item) => item.built && item.href !== "/dashboard");

  return (
    <>
      <PageHeader
        title={`Welcome, ${me.display_name.split(" ")[0]}`}
        description={
          me.student
            ? `${me.student.roll_no} · ${me.student.program_name} · Semester ${me.student.current_semester}`
            : me.department
              ? `${ROLE_LABELS[me.role]} · ${me.department.name}`
              : `Signed in as ${ROLE_LABELS[me.role].toLowerCase()}`
        }
      />
      <div className="grid gap-4 md:grid-cols-2 2xl:grid-cols-3">
        {me.student && <AttendanceSummary studentId={me.student.id} />}
        {me.student && <FeesDue />}
        {me.department && <DepartmentSummary department={me.department} />}
        <LatestNotices />
        <Card>
          <CardHeader>
            <CardTitle>In this demo</CardTitle>
            <CardDescription>The pages open to you. The full product has more; see the README.</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-wrap gap-2">
            {built.map((item) => (
              <Link key={item.href} href={item.href}>
                <Badge variant="secondary" className="gap-1">
                  <item.icon aria-hidden className="size-3" />
                  {item.title}
                </Badge>
              </Link>
            ))}
          </CardContent>
        </Card>
      </div>
    </>
  );
}

async function AttendanceSummary({ studentId }: { studentId: string }) {
  try {
    const report = await getAttendance(studentId);
    const shortages = report.subjects.filter((subject) => subject.shortage).length;
    return (
      <Card>
        <CardHeader>
          <CardTitle>Attendance</CardTitle>
          <CardDescription>This academic year, all subjects</CardDescription>
        </CardHeader>
        <CardContent className="flex items-end justify-between gap-4">
          <div>
            <div className="text-4xl font-semibold tabular-nums">{report.overall.percentage.toFixed(1)}%</div>
            <div className="text-muted-foreground text-sm">
              {shortages === 0 ? "No subject in shortage" : `${shortages} subject${shortages > 1 ? "s" : ""} in shortage`}
            </div>
          </div>
          <Link href="/academic/attendance" className="text-sm underline-offset-4 hover:underline">
            Details
          </Link>
        </CardContent>
      </Card>
    );
  } catch (err) {
    return <ApiProblem error={err} />;
  }
}

async function LatestNotices() {
  try {
    const { notices, unread } = await getNotices();
    return (
      <Card className="md:row-span-2">
        <CardHeader>
          <CardTitle>Latest notices</CardTitle>
          <CardDescription>{unread === 0 ? "All read" : `${unread} unread`}</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-2">
          {notices.slice(0, 3).map((notice) => (
            <NoticeCard key={notice.id} notice={notice} audience={notice.program_name} unread={!notice.read} compact />
          ))}
          <Link href="/notices" className="text-sm underline-offset-4 hover:underline">
            All notices
          </Link>
        </CardContent>
      </Card>
    );
  } catch (err) {
    return <ApiProblem error={err} />;
  }
}

async function FeesDue() {
  try {
    const { lines, totals } = await getPayable();
    const next = lines
      .filter((line) => line.pending > 0 && line.due_on)
      .sort((a, b) => a.due_on!.localeCompare(b.due_on!))[0];
    return (
      <Card>
        <CardHeader>
          <CardTitle>Fees</CardTitle>
          <CardDescription>
            {totals.pending > 0 && next
              ? `Next due ${new Date(next.due_on!).toLocaleDateString("en-IN", { dateStyle: "medium" })}: ${next.particulars}`
              : "Nothing due"}
          </CardDescription>
        </CardHeader>
        <CardContent className="flex items-end justify-between gap-4">
          <div>
            <div className="text-4xl font-semibold tabular-nums">{formatPaise(Math.max(totals.pending, 0))}</div>
            <div className="text-muted-foreground text-sm">outstanding</div>
          </div>
          <Link href="/fees/payable" className="text-sm underline-offset-4 hover:underline">
            {totals.pending > 0 ? "Pay" : "Details"}
          </Link>
        </CardContent>
      </Card>
    );
  } catch (err) {
    return <ApiProblem error={err} />;
  }
}

function DepartmentSummary({ department }: { department: Department }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{department.name}</CardTitle>
        <CardDescription>{department.programs.map((p) => p.name).join(", ") || "No programs"}</CardDescription>
      </CardHeader>
      <CardContent className="flex gap-8">
        <div>
          <div className="text-4xl font-semibold tabular-nums">{department.students}</div>
          <div className="text-muted-foreground text-sm">students</div>
        </div>
        <div>
          <div className="text-4xl font-semibold tabular-nums">{department.faculty}</div>
          <div className="text-muted-foreground text-sm">faculty</div>
        </div>
      </CardContent>
    </Card>
  );
}

