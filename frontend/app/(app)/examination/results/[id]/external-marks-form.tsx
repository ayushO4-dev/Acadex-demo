"use client";

import { useState, useTransition } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import type { ExternalSheet } from "@/lib/api";

import { saveExternalMarks } from "../actions";

/** Valid marks: 0 to max, in halves. Blank is "not entered yet". */
export function validMarks(value: string, max: number): boolean {
  const n = Number(value);
  return value.trim() !== "" && Number.isFinite(n) && n >= 0 && n <= max && Number.isInteger(n * 2);
}

export function ExternalMarksForm({
  setId,
  offeringId,
  sheet,
  locked,
}: {
  setId: string;
  offeringId: string;
  sheet: ExternalSheet;
  locked: boolean;
}) {
  const [values, setValues] = useState<Record<string, string>>(() =>
    Object.fromEntries(sheet.students.map((s) => [s.student_id, s.marks === null ? "" : String(s.marks)])),
  );
  const [pending, startTransition] = useTransition();
  const filled = sheet.students.filter((s) => values[s.student_id].trim() !== "");
  const invalid = filled.filter((s) => !validMarks(values[s.student_id], sheet.max_marks));

  function save() {
    startTransition(async () => {
      const result = await saveExternalMarks(
        setId,
        offeringId,
        filled.map((s) => ({ student_id: s.student_id, attempt: s.attempt, marks: Number(values[s.student_id]) })),
      );
      if (result.ok) toast.success(`Saved marks for ${result.saved} students`);
      else toast.error(result.message);
    });
  }

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-muted-foreground text-sm">
          {filled.length} of {sheet.students.length} entered
          {invalid.length > 0 && <span className="text-destructive"> · {invalid.length} out of range</span>}
        </p>
        {!locked && (
          <Button size="sm" onClick={save} disabled={pending || invalid.length > 0 || filled.length === 0}>
            {pending ? "Saving…" : "Save marks"}
          </Button>
        )}
      </div>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Roll no.</TableHead>
            <TableHead>Name</TableHead>
            <TableHead className="text-right">Attempt</TableHead>
            <TableHead className="w-32 text-right">Marks /{sheet.max_marks}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody className="tabular-nums">
          {sheet.students.map((s) => {
            const value = values[s.student_id];
            const bad = value.trim() !== "" && !validMarks(value, sheet.max_marks);
            return (
              <TableRow key={s.student_id}>
                <TableCell className="font-mono text-xs">{s.roll_no}</TableCell>
                <TableCell>{s.display_name}</TableCell>
                <TableCell className="text-right">{s.attempt}</TableCell>
                <TableCell className="text-right">
                  <Input
                    inputMode="decimal"
                    aria-label={`Marks for ${s.display_name}`}
                    aria-invalid={bad}
                    disabled={locked}
                    value={value}
                    onChange={(e) => setValues((v) => ({ ...v, [s.student_id]: e.target.value }))}
                    className="h-8 text-right"
                  />
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
    </div>
  );
}
