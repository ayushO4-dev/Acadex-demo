import { ArrowLeft } from "lucide-react";
import Link from "next/link";

import { ApiProblem } from "@/components/api-problem";
import { PageHeader } from "@/components/page-header";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { getExternalSheet, getMe, getSetSubjects } from "@/lib/acadex";
import type { ExternalSheet, SetSubject } from "@/lib/api";
import { academicYear, cn } from "@/lib/utils";

import { KIND_LABEL } from "../labels";
import { PublicationActions } from "../publication-controls";
import { ExternalMarksForm } from "./external-marks-form";

export const dynamic = "force-dynamic";

/** A result set's external marks, one subject at a time. */
export default async function ResultSetPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<{ subject?: string }>;
}) {
  const [{ id }, { subject: wanted }, me] = await Promise.all([params, searchParams, getMe()]);
  let subjects: SetSubject[];
  let sheet: ExternalSheet | null = null;
  let subject: SetSubject | undefined;
  try {
    subjects = await getSetSubjects(id);
    subject = subjects.find((s) => s.offering_id === wanted) ?? subjects[0];
    if (subject) sheet = await getExternalSheet(id, subject.offering_id);
  } catch (err) {
    return (
      <>
        <PageHeader title="Result set" />
        <ApiProblem error={err} />
      </>
    );
  }
  if (!subject || !sheet) {
    return (
      <>
        <PageHeader title="Result set" />
        <p className="text-muted-foreground">This result set has no subjects.</p>
      </>
    );
  }

  const p = sheet.publication;
  // Re-evaluation marks come from decisions on requests, not from here.
  const locked = p.published_at !== null || p.requested_at !== null || p.kind === "revaluation";
  return (
    <>
      <Link href="/examination/results" className="text-muted-foreground mb-2 inline-flex items-center gap-1 text-sm hover:underline">
        <ArrowLeft className="size-4" aria-hidden />
        Results
      </Link>
      <PageHeader
        title={`${p.program_name}, semester ${p.semester}`}
        description={`${KIND_LABEL[p.kind]} results · academic year ${academicYear(p.academic_year)} · ${p.students} students with marks in ${p.subjects} of ${subjects.length} subjects`}
        actions={
          <>
            <PublicationActions publication={p} meId={me.id} />
          </>
        }
      />

      <nav aria-label="Subject" className="mb-4 flex flex-wrap gap-1">
        {subjects.map((s) => (
          <Link
            key={s.offering_id}
            href={`/examination/results/${id}?subject=${s.offering_id}`}
            aria-current={s.offering_id === subject.offering_id ? "page" : undefined}
            className={cn(
              "rounded-md border px-3 py-1.5 text-sm",
              s.offering_id === subject.offering_id ? "bg-primary text-primary-foreground border-primary" : "hover:bg-muted",
            )}
          >
            {s.code}
          </Link>
        ))}
      </nav>

      <Card>
        <CardHeader>
          <CardTitle className="flex flex-wrap items-center gap-2">
            {subject.code} {subject.name}
            {p.published_at ? (
              <Badge variant="outline">Published</Badge>
            ) : p.requested_at ? (
              <Badge>Awaiting approval</Badge>
            ) : null}
          </CardTitle>
          <CardDescription>
            External exam out of {sheet.max_marks}, in half marks.{" "}
            {p.kind === "backlog"
              ? "Students who registered this subject on a backlog form; the attempt is their latest plus one."
              : p.kind === "revaluation"
                ? "Marks changed by re-evaluation decisions; they're set from the requests list."
                : "Every student in the batch."}
            {p.requested_at && !p.published_at && " Marks are frozen while publishing is requested; withdraw the request to change one."}
          </CardDescription>
        </CardHeader>
        <CardContent>
          {sheet.students.length === 0 ? (
            <p className="text-muted-foreground text-sm">Nobody sits this paper in this result set.</p>
          ) : (
            <ExternalMarksForm key={subject.offering_id} setId={id} offeringId={subject.offering_id} sheet={sheet} locked={locked} />
          )}
        </CardContent>
      </Card>
    </>
  );
}
