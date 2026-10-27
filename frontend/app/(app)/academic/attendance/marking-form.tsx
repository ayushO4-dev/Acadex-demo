"use client";

import { useState, useTransition } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import type { AttendanceStatus, Roster, SessionKind } from "@/lib/api";
import { cn } from "@/lib/utils";

import { saveAttendance } from "./actions";

const STATUSES: { value: AttendanceStatus; label: string; short: string }[] = [
  { value: "present", label: "Present", short: "P" },
  { value: "absent", label: "Absent", short: "A" },
  { value: "late", label: "Late", short: "L" },
  { value: "excused", label: "Excused", short: "E" },
];

/**
 * One row per student with a status toggle. Unmarked students start as
 * present, so a typical class needs only the absentees clicked.
 */
export function MarkingForm({ roster, sessionKind }: { roster: Roster; sessionKind: SessionKind }) {
  const [marks, setMarks] = useState<Record<string, AttendanceStatus>>(() =>
    Object.fromEntries(roster.students.map((s) => [s.student_id, s.status ?? "present"])),
  );
  const [pending, startTransition] = useTransition();
  const alreadyMarked = roster.students.some((s) => s.status !== null);
  const counts = STATUSES.map((s) => ({
    ...s,
    count: Object.values(marks).filter((value) => value === s.value).length,
  }));

  function save() {
    startTransition(async () => {
      const result = await saveAttendance(
        roster.offering.id,
        roster.session_date,
        roster.slot,
        sessionKind,
        Object.entries(marks).map(([student_id, status]) => ({ student_id, status })),
      );
      if (result.ok) {
        toast.success(`Saved attendance for ${result.saved} students`);
      } else {
        toast.error(result.message);
      }
    });
  }

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-muted-foreground text-sm">
          {counts.map((s) => `${s.count} ${s.label.toLowerCase()}`).join(" · ")}
          {alreadyMarked && " · editing a session already marked"}
        </p>
        <div className="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() =>
              setMarks(Object.fromEntries(roster.students.map((s) => [s.student_id, "present" as const])))
            }
          >
            All present
          </Button>
          <Button size="sm" onClick={save} disabled={pending}>
            {pending ? "Saving…" : "Save attendance"}
          </Button>
        </div>
      </div>

      <ul className="divide-y rounded-md border">
        {roster.students.map((student) => (
          <li key={student.student_id} className="flex items-center justify-between gap-3 px-3 py-2">
            <div className="min-w-0">
              <div className="truncate font-medium">{student.display_name}</div>
              <div className="text-muted-foreground text-xs">{student.roll_no}</div>
            </div>
            <div role="radiogroup" aria-label={`Attendance for ${student.display_name}`} className="flex gap-1">
              {STATUSES.map((status) => {
                const selected = marks[student.student_id] === status.value;
                return (
                  <button
                    key={status.value}
                    type="button"
                    role="radio"
                    aria-checked={selected}
                    aria-label={status.label}
                    title={status.label}
                    onClick={() => setMarks((m) => ({ ...m, [student.student_id]: status.value }))}
                    className={cn(
                      "size-8 rounded-md border text-sm font-medium transition-colors",
                      selected
                        ? status.value === "absent"
                          ? "bg-destructive border-destructive text-white"
                          : "bg-primary border-primary text-primary-foreground"
                        : "hover:bg-muted",
                    )}
                  >
                    {status.short}
                  </button>
                );
              })}
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}
