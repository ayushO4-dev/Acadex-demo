"use client";

import { Plus, Trash2 } from "lucide-react";
import { useState, useTransition } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import type { InternalSheet, MarksComponent } from "@/lib/api";

import { saveComponents, saveInternalMarks } from "../../actions";
import { validMarks } from "../../[id]/external-marks-form";

type Row = { id: string | null; code: string; max_marks: string };

export function ComponentsEditor({
  offeringId,
  components,
  locked,
}: {
  offeringId: string;
  components: MarksComponent[];
  locked: boolean;
}) {
  const [rows, setRows] = useState<Row[]>(() =>
    components.length > 0
      ? components.map((c) => ({ id: c.id, code: c.code, max_marks: String(c.max_marks) }))
      : [
          { id: null, code: "CCA1", max_marks: "10" },
          { id: null, code: "CCA2", max_marks: "10" },
          { id: null, code: "MID", max_marks: "20" },
        ],
  );
  const [pending, startTransition] = useTransition();
  const set = (i: number, patch: Partial<Row>) => setRows((r) => r.map((row, j) => (j === i ? { ...row, ...patch } : row)));
  const total = rows.reduce((sum, r) => sum + (Number(r.max_marks) || 0), 0);

  function save() {
    startTransition(async () => {
      const result = await saveComponents(
        offeringId,
        rows.map((r) => ({ id: r.id, code: r.code.trim(), max_marks: Number(r.max_marks) })),
      );
      if (result.ok) toast.success("Components saved");
      else toast.error(result.message);
    });
  }

  return (
    <div className="space-y-3">
      <ul className="grid gap-2">
        {rows.map((row, i) => (
          <li key={row.id ?? `new-${i}`} className="flex items-center gap-2">
            <Input
              aria-label="Code"
              value={row.code}
              disabled={locked}
              maxLength={12}
              onChange={(e) => set(i, { code: e.target.value.toUpperCase() })}
              className="w-32 font-mono"
            />
            <Input
              aria-label={`Maximum marks for ${row.code}`}
              inputMode="numeric"
              value={row.max_marks}
              disabled={locked}
              onChange={(e) => set(i, { max_marks: e.target.value })}
              className="w-24 text-right"
            />
            <span className="text-muted-foreground text-sm">marks</span>
            {!locked && (
              <Button
                variant="ghost"
                size="icon"
                aria-label={`Remove ${row.code}`}
                onClick={() => setRows((r) => r.filter((_, j) => j !== i))}
                disabled={rows.length === 1}
              >
                <Trash2 />
              </Button>
            )}
          </li>
        ))}
      </ul>
      {!locked && (
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => setRows((r) => [...r, { id: null, code: "", max_marks: "10" }])}
            disabled={rows.length >= 8}
          >
            <Plus />
            Add component
          </Button>
          <Button size="sm" onClick={save} disabled={pending || total > 100}>
            {pending ? "Saving…" : "Save components"}
          </Button>
          <span className={total > 100 ? "text-destructive text-sm" : "text-muted-foreground text-sm"}>{total} / 100</span>
        </div>
      )}
    </div>
  );
}

/** A grid: one row per student, one column per component. */
export function InternalMarksForm({ offeringId, sheet }: { offeringId: string; sheet: InternalSheet }) {
  const components = sheet.components.filter((c): c is MarksComponent & { id: string } => c.id !== null);
  const key = (student: string, component: string) => `${student}:${component}`;
  const [values, setValues] = useState<Record<string, string>>(() =>
    Object.fromEntries(
      sheet.students.flatMap((s) =>
        components.map((c) => [key(s.student_id, c.id), s.marks[c.id] === undefined ? "" : String(s.marks[c.id])]),
      ),
    ),
  );
  const [pending, startTransition] = useTransition();
  const cells = sheet.students.flatMap((s) =>
    components.map((c) => ({ student_id: s.student_id, component: c, value: values[key(s.student_id, c.id)] ?? "" })),
  );
  const filled = cells.filter((c) => c.value.trim() !== "");
  const invalid = filled.filter((c) => !validMarks(c.value, c.component.max_marks));

  function save() {
    startTransition(async () => {
      const result = await saveInternalMarks(
        offeringId,
        filled.map((c) => ({ student_id: c.student_id, component_id: c.component.id, marks: Number(c.value) })),
      );
      if (result.ok) toast.success(`Saved ${result.saved} marks`);
      else toast.error(result.message);
    });
  }

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-muted-foreground text-sm">
          {filled.length} of {cells.length} entered
          {invalid.length > 0 && <span className="text-destructive"> · {invalid.length} out of range</span>}
        </p>
        {!sheet.published && (
          <Button size="sm" onClick={save} disabled={pending || invalid.length > 0 || filled.length === 0}>
            {pending ? "Saving…" : "Save marks"}
          </Button>
        )}
      </div>
      <div className="overflow-x-auto">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Student</TableHead>
              {components.map((c) => (
                <TableHead key={c.id} className="w-24 text-right">
                  {c.code} <span className="text-muted-foreground font-normal">/{c.max_marks}</span>
                </TableHead>
              ))}
              <TableHead className="text-right">Total</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody className="tabular-nums">
            {sheet.students.map((s) => {
              const total = components.reduce((sum, c) => sum + (Number(values[key(s.student_id, c.id)]) || 0), 0);
              return (
                <TableRow key={s.student_id}>
                  <TableCell>
                    <div className="font-medium">{s.display_name}</div>
                    <div className="text-muted-foreground font-mono text-xs">{s.roll_no}</div>
                  </TableCell>
                  {components.map((c) => {
                    const k = key(s.student_id, c.id);
                    const value = values[k] ?? "";
                    return (
                      <TableCell key={c.id} className="text-right">
                        <Input
                          inputMode="decimal"
                          aria-label={`${c.code} for ${s.display_name}`}
                          aria-invalid={value.trim() !== "" && !validMarks(value, c.max_marks)}
                          disabled={sheet.published}
                          value={value}
                          onChange={(e) => setValues((v) => ({ ...v, [k]: e.target.value }))}
                          className="h-8 text-right"
                        />
                      </TableCell>
                    );
                  })}
                  <TableCell className="text-right font-medium">{total}</TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      </div>
    </div>
  );
}
