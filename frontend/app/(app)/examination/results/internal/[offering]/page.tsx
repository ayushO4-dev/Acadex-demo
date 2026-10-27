import { ArrowLeft } from "lucide-react";
import Link from "next/link";

import { ApiProblem } from "@/components/api-problem";
import { PageHeader } from "@/components/page-header";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { getInternalSheet, getOfferings } from "@/lib/acadex";
import type { InternalSheet } from "@/lib/api";

import { ComponentsEditor, InternalMarksForm } from "./internal-forms";

export const dynamic = "force-dynamic";

export default async function InternalMarksPage({ params }: { params: Promise<{ offering: string }> }) {
  const { offering: id } = await params;
  let sheet: InternalSheet;
  try {
    sheet = await getInternalSheet(id);
  } catch (err) {
    return (
      <>
        <PageHeader title="Internal marks" />
        <ApiProblem error={err} />
      </>
    );
  }
  const offering = (await getOfferings().catch(() => [])).find((o) => o.id === id);
  const total = sheet.components.reduce((sum, c) => sum + c.max_marks, 0);
  // Remount the forms when the saved components change, so new ones get ids.
  const layout = sheet.components.map((c) => `${c.id}/${c.code}/${c.max_marks}`).join(",");

  return (
    <>
      <Link href="/examination/results" className="text-muted-foreground mb-2 inline-flex items-center gap-1 text-sm hover:underline">
        <ArrowLeft className="size-4" aria-hidden />
        Internal marks
      </Link>
      <PageHeader
        title={offering ? `${offering.code} ${offering.name}` : "Internal marks"}
        description={offering ? `${offering.program_name} · semester ${offering.semester} · ${sheet.students.length} students` : undefined}
      />
      {sheet.published && (
        <Alert className="mb-4">
          <AlertTitle>Results published</AlertTitle>
          <AlertDescription>This subject&apos;s results are published, so its internal marks are locked.</AlertDescription>
        </Alert>
      )}
      <div className="grid gap-4">
        <Card>
          <CardHeader>
            <CardTitle>Assessment components</CardTitle>
            <CardDescription>
              For example two CCAs and a midterm, or lab assessments and a viva. Up to 100 marks in all
              {total > 0 && ` (now ${total})`}.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <ComponentsEditor key={layout} offeringId={id} components={sheet.components} locked={sheet.published} />
          </CardContent>
        </Card>
        {sheet.components.length === 0 && (
          <p className="text-muted-foreground text-sm">Add the assessment components above; the marks sheet appears here.</p>
        )}
        {sheet.components.length > 0 && sheet.students.length === 0 && (
          <p className="text-muted-foreground text-sm">No students are in this class yet.</p>
        )}
        {sheet.components.length > 0 && sheet.students.length > 0 && (
          <Card>
            <CardHeader>
              <CardTitle>Marks</CardTitle>
              <CardDescription>Half marks are fine. Leave a cell blank if the assessment hasn&apos;t happened yet.</CardDescription>
            </CardHeader>
            <CardContent>
              <InternalMarksForm key={layout} offeringId={id} sheet={sheet} />
            </CardContent>
          </Card>
        )}
      </div>
    </>
  );
}
