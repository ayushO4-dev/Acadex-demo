import { Download } from "lucide-react";
import Link from "next/link";

import { ApiProblem } from "@/components/api-problem";
import { PageHeader } from "@/components/page-header";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableFooter,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { asUser, getMe, getOfferings, getPublications, getResults } from "@/lib/acadex";
import type { Offering, Publication, ResultKind, ResultsReport, SubjectResult } from "@/lib/api";
import { academicYear, cn } from "@/lib/utils";

import { KIND_LABEL } from "./labels";
import { NewSetDialog, PublicationActions } from "./publication-controls";
import { SemesterSelect } from "./semester-select";

export const dynamic = "force-dynamic";

const DATE = new Intl.DateTimeFormat("en-IN", { dateStyle: "medium", timeZone: "Asia/Kolkata" });

const VIEWS: ResultKind[] = ["regular", "backlog", "revaluation"];

export default async function ResultsPage({
  searchParams,
}: {
  searchParams: Promise<{ semester?: string; view?: string }>;
}) {
  const me = await getMe();
  if (me.permissions.includes("results:publish")) return <Publications meId={me.id} />;
  if (me.permissions.includes("marks:enter")) return <MarksEntry />;
  if (!me.student) {
    return (
      <>
        <PageHeader title="Results" />
        <p className="text-muted-foreground">Results are visible to students and the exam cell.</p>
      </>
    );
  }

  const search = await searchParams;
  const semester = Number(search.semester) || undefined;
  const view = VIEWS.find((v) => v === search.view) ?? "regular";
  let report: ResultsReport;
  try {
    report = await getResults(me.student.id, semester);
  } catch (err) {
    return (
      <>
        <PageHeader title="Results" />
        <ApiProblem error={err} />
      </>
    );
  }
  return <StudentResults report={report} requested={semester} view={view} />;
}

function StudentResults({ report, requested, view }: { report: ResultsReport; requested?: number; view: ResultKind }) {
  const { semesters, selected } = report;
  if (semesters.length === 0) {
    return (
      <>
        <PageHeader title="Results" />
        <p className="text-muted-foreground">No results have been published for you yet.</p>
      </>
    );
  }

  const current = selected?.semester ?? requested ?? semesters[semesters.length - 1].semester;
  // Regular: each subject's first attempt, as first published. Backlog and
  // re-evaluation: the later attempts. SGPA, CGPA and credits always count
  // each subject's latest result.
  const rows = selected ? selected.history.filter((s) => (view === "regular" ? s.attempt === 1 : s.kind === view)) : [];
  const clearedLater = selected ? selected.passed && rows.some((s) => !s.passed) : false;
  return (
    <>
      <PageHeader
        title="Results"
        description={
          selected ? `${KIND_LABEL[view]} examination · semester ${selected.semester} · academic year ${academicYear(selected.academic_year)}` : undefined
        }
        actions={
          <>
            <div className="flex flex-wrap items-center gap-2">
              <nav aria-label="Examination" className="inline-flex h-8 items-center rounded-md border p-0.5">
                {VIEWS.map((v) => (
                  <Link
                    key={v}
                    href={`/examination/results?semester=${current}&view=${v}`}
                    aria-current={v === view ? "page" : undefined}
                    className={cn(
                      "grid h-full place-items-center rounded px-3 text-sm",
                      v === view ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:bg-muted",
                    )}
                  >
                    {KIND_LABEL[v]}
                  </Link>
                ))}
              </nav>
              <SemesterSelect semesters={semesters} value={current} />
              {selected?.published_at && (
                <Button asChild variant="outline">
                  <a href={`/examination/results/grade-card/${selected.semester}`}>
                    <Download />
                    Grade card
                  </a>
                </Button>
              )}
            </div>
          </>
        }
      />

      {!selected ? (
        <p className="text-muted-foreground">Semester {current} results haven&apos;t been published yet.</p>
      ) : (
        <>
          <div className="mb-4 grid grid-cols-2 gap-4 md:grid-cols-4">
            {view === "regular" ? (
              <Stat
                label="SGPA"
                value={selected.sgpa_regular.toFixed(2)}
                hint={selected.sgpa !== selected.sgpa_regular ? `${selected.sgpa.toFixed(2)} after backlogs` : undefined}
              />
            ) : (
              <Stat label="SGPA" value={selected.sgpa.toFixed(2)} hint={`Was ${selected.sgpa_regular.toFixed(2)} in the regular exam`} />
            )}
            <Stat label="CGPA" value={selected.cgpa.toFixed(2)} hint={`Semesters 1–${selected.semester}, latest results`} />
            <Stat label="Credits earned" value={`${selected.credits_earned} / ${selected.credits_attempted}`} />
            <Stat
              label="Result"
              value={selected.passed ? "Pass" : "Backlog"}
              hint={
                selected.passed
                  ? clearedLater
                    ? "Backlogs cleared"
                    : undefined
                  : `${selected.subjects.filter((s) => !s.passed).length} subject(s) to clear`
              }
              danger={!selected.passed}
            />
          </div>

          {rows.length === 0 ? (
            <Card className="mb-4">
              <CardContent className="text-muted-foreground text-sm">
                No {KIND_LABEL[view].toLowerCase()} results for semester {selected.semester}.
              </CardContent>
            </Card>
          ) : (
          <Card className="mb-4">
            <CardHeader>
              <CardTitle>Result sheet</CardTitle>
              <CardDescription>
                Total is internal out of 40 plus external out of 60. Passing needs a P grade or better and at least 40%
                in the external exam.
              </CardDescription>
            </CardHeader>
            <CardContent>
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Subject</TableHead>
                    {view !== "regular" && <TableHead>Published</TableHead>}
                    <TableHead className="text-right">Credits</TableHead>
                    <TableHead className="text-right">Internal</TableHead>
                    <TableHead className="text-right">External</TableHead>
                    <TableHead className="text-right">Total</TableHead>
                    <TableHead className="text-center">Grade</TableHead>
                    <TableHead className="text-right">Points</TableHead>
                    <TableHead>Result</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody className="tabular-nums">
                  {rows.map((s) => {
                    const latest = selected.subjects.find((l) => l.code === s.code);
                    const superseded = latest !== undefined && latest.attempt !== s.attempt;
                    return (
                    <TableRow key={`${s.code}-${s.attempt}`}>
                      <TableCell>
                        <span className="font-medium">{s.code}</span>{" "}
                        <span className="text-muted-foreground">{s.name}</span>
                      </TableCell>
                      {view !== "regular" && (
                        <TableCell>{s.published_at ? DATE.format(new Date(s.published_at)) : "Staged"}</TableCell>
                      )}
                      <TableCell className="text-right">{s.credits}</TableCell>
                      <TableCell className="text-right">{s.internal}</TableCell>
                      <TableCell className="text-right">{s.external}</TableCell>
                      <TableCell className="text-right">
                        {s.total}
                        <span className="text-muted-foreground">/{s.max}</span>
                      </TableCell>
                      <TableCell className="text-center font-semibold">{s.grade}</TableCell>
                      <TableCell className="text-right">{s.points}</TableCell>
                      <TableCell>
                        {s.passed ? <Badge variant="outline">Pass</Badge> : <Badge variant="destructive">Fail</Badge>}
                        {superseded && latest.passed && (
                          <span className="text-muted-foreground ml-2 text-xs">cleared in {KIND_LABEL[latest.kind].toLowerCase()}</span>
                        )}
                      </TableCell>
                    </TableRow>
                    );
                  })}
                </TableBody>
                <TableFooter>
                  <TableRow>
                    <TableCell>SGPA {(view === "regular" ? selected.sgpa_regular : selected.sgpa).toFixed(2)}</TableCell>
                    {view !== "regular" && <TableCell />}
                    <TableCell className="text-right tabular-nums">{rows.reduce((sum, s) => sum + s.credits, 0)}</TableCell>
                    <TableCell colSpan={6} />
                  </TableRow>
                </TableFooter>
              </Table>
            </CardContent>
          </Card>
          )}

          <InternalMarks subjects={selected.subjects} />
        </>
      )}
    </>
  );
}

/**
 * Internal assessment, one table per component layout: columns come from each
 * offering's configured components (theory: CCAs and a midterm; labs: LCAs
 * and a viva), not from a fixed list (architecture.md §9.3).
 */
function InternalMarks({ subjects }: { subjects: SubjectResult[] }) {
  const layouts = new Map<string, SubjectResult[]>();
  for (const subject of subjects) {
    const key = subject.components.map((c) => `${c.code}/${c.max}`).join(",");
    layouts.set(key, [...(layouts.get(key) ?? []), subject]);
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Internal assessment</CardTitle>
        <CardDescription>Continuous assessment, labs and midterms, as configured for each subject.</CardDescription>
      </CardHeader>
      <CardContent className="grid gap-6">
        {[...layouts.values()].map((group) => (
          <Table key={group[0].code}>
            <TableHeader>
              <TableRow>
                <TableHead>Subject</TableHead>
                {group[0].components.map((c) => (
                  <TableHead key={c.code} className="text-right">
                    {c.code} <span className="text-muted-foreground font-normal">/{c.max}</span>
                  </TableHead>
                ))}
                <TableHead className="text-right">
                  Total <span className="text-muted-foreground font-normal">/{group[0].internal_max}</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody className="tabular-nums">
              {group.map((s) => (
                <TableRow key={s.code}>
                  <TableCell>
                    <span className="font-medium">{s.code}</span>{" "}
                    <span className="text-muted-foreground">{s.name}</span>
                  </TableCell>
                  {s.components.map((c) => (
                    <TableCell key={c.code} className="text-right">
                      {c.marks ?? "—"}
                    </TableCell>
                  ))}
                  <TableCell className="text-right font-medium">{s.internal}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        ))}
      </CardContent>
    </Card>
  );
}

function Stat({ label, value, hint, danger = false }: { label: string; value: string; hint?: string; danger?: boolean }) {
  return (
    <Card>
      <CardHeader>
        <CardDescription>{label}</CardDescription>
        <CardTitle className={danger ? "text-destructive text-2xl tabular-nums" : "text-2xl tabular-nums"}>
          {value}
        </CardTitle>
        {hint && <p className="text-muted-foreground text-xs">{hint}</p>}
      </CardHeader>
    </Card>
  );
}

async function Publications({ meId }: { meId: string }) {
  let publications: Publication[];
  let programs: { id: number; name: string }[];
  try {
    [publications, programs] = await Promise.all([
      getPublications(),
      asUser<{ id: number; name: string }[]>("/programs"),
    ]);
  } catch (err) {
    return (
      <>
        <PageHeader title="Results" />
        <ApiProblem error={err} />
      </>
    );
  }

  return (
    <>
      <PageHeader
        title="Results"
        description="Staged results are invisible to students. One person in the exam cell requests publishing; someone else approves it, and the program is notified."
        actions={
          <>
            <NewSetDialog programs={programs} />
          </>
        }
      />
      <Card>
        <CardContent>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Program</TableHead>
                <TableHead>Semester</TableHead>
                <TableHead>Kind</TableHead>
                <TableHead>Academic year</TableHead>
                <TableHead className="text-right">Subjects</TableHead>
                <TableHead className="text-right">Students</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="text-right">
                  <span className="sr-only">Action</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody className="tabular-nums">
              {publications.map((p) => (
                <TableRow key={p.id}>
                  <TableCell className="font-medium">{p.program_name}</TableCell>
                  <TableCell>{p.semester}</TableCell>
                  <TableCell>{KIND_LABEL[p.kind]}</TableCell>
                  <TableCell>{academicYear(p.academic_year)}</TableCell>
                  <TableCell className="text-right">{p.subjects}</TableCell>
                  <TableCell className="text-right">{p.students}</TableCell>
                  <TableCell className="whitespace-normal">
                    {p.published_at && p.withdrawal_requested_by ? (
                      <span className="text-sm">
                        <Badge variant="destructive">Withdrawal requested</Badge>{" "}
                        <span className="text-muted-foreground">
                          {p.withdrawal_requested_by_id === meId ? "by you" : `by ${p.withdrawal_requested_by}`}: {p.withdrawal_reason}
                        </span>
                      </span>
                    ) : p.published_at ? (
                      <span className="text-sm">
                        <Badge variant="outline">Published</Badge>{" "}
                        <span className="text-muted-foreground">
                          {DATE.format(new Date(p.published_at))}
                          {p.published_by && ` by ${p.published_by}`}
                          {p.requested_by && `, requested by ${p.requested_by}`}
                        </span>
                      </span>
                    ) : p.requested_at ? (
                      <span className="text-sm">
                        <Badge>Awaiting approval</Badge>{" "}
                        <span className="text-muted-foreground">
                          {p.requested_by_id === meId ? "requested by you" : `requested by ${p.requested_by}`}
                        </span>
                      </span>
                    ) : (
                      <span className="text-sm">
                        <Badge variant="secondary">Staged</Badge>
                        {p.withdrawals > 0 && <span className="text-muted-foreground"> withdrawn for correction</span>}
                      </span>
                    )}
                  </TableCell>
                  <TableCell className="text-right">
                    <PublicationActions publication={p} meId={meId} />
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </>
  );
}

/** Faculty: internal marks for the subjects they teach this year. */
async function MarksEntry() {
  let offerings: Offering[];
  try {
    offerings = await getOfferings();
  } catch (err) {
    return (
      <>
        <PageHeader title="Results" />
        <ApiProblem error={err} />
      </>
    );
  }
  return (
    <>
      <PageHeader
        title="Internal marks"
        description="Set each subject's assessment components and enter marks. They lock when the exam cell publishes the subject's results."
      />
      {offerings.length === 0 ? (
        <p className="text-muted-foreground">You aren&apos;t teaching any subjects this year.</p>
      ) : (
        <ul className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4">
          {offerings.map((o) => (
            <li key={o.id}>
              <Link href={`/examination/results/internal/${o.id}`} className="block">
                <Card className="hover:bg-muted/50 transition-colors">
                  <CardHeader>
                    <CardTitle>
                      {o.code} {o.name}
                    </CardTitle>
                    <CardDescription>
                      {o.program_name} · semester {o.semester} · {o.student_count} students
                    </CardDescription>
                  </CardHeader>
                </Card>
              </Link>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}
