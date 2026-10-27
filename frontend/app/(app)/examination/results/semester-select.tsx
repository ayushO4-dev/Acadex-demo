"use client";

import { usePathname, useRouter, useSearchParams } from "next/navigation";

import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { SemesterSummary } from "@/lib/api";

/** The semester in the URL, so a result sheet can be linked to. */
export function SemesterSelect({ semesters, value }: { semesters: SemesterSummary[]; value: number }) {
  const router = useRouter();
  const pathname = usePathname();
  const search = useSearchParams();
  // Keeps the Regular / Backlog / Re-evaluation choice.
  const view = search.get("view");
  return (
    <Select
      value={String(value)}
      onValueChange={(semester) => router.push(`${pathname}?semester=${semester}${view ? `&view=${view}` : ""}`)}
    >
      <SelectTrigger className="w-44" aria-label="Semester">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {semesters.map((s) => (
          <SelectItem key={s.semester} value={String(s.semester)}>
            Semester {s.semester}
            {s.published_at === null && " (staged)"}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
