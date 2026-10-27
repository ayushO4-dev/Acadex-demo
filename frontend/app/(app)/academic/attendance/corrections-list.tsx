import { ApiProblem } from "@/components/api-problem";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { getCorrections } from "@/lib/acadex";
import type { Correction } from "@/lib/api";

import { DecideCorrection } from "./correction-dialogs";

const DATE = new Intl.DateTimeFormat("en-IN", { dateStyle: "medium", timeZone: "UTC" });

/** Correction requests the viewer may see; an HOD can decide pending ones. */
export async function CorrectionsList({ canDecide, title, description }: { canDecide: boolean; title: string; description: string }) {
  let corrections: Correction[];
  try {
    corrections = await getCorrections();
  } catch (err) {
    return <ApiProblem error={err} />;
  }
  const pending = corrections.filter((c) => c.status === "pending").length;

  return (
    <Card>
      <CardHeader>
        <CardTitle>
          {title}
          {pending > 0 && <Badge className="ml-2 align-middle">{pending} pending</Badge>}
        </CardTitle>
        <CardDescription>{description}</CardDescription>
      </CardHeader>
      <CardContent>
        {corrections.length === 0 ? (
          <p className="text-muted-foreground text-sm">None.</p>
        ) : (
          <ul className="divide-y">
            {corrections.map((c) => {
              const session = `${c.subject_code}, ${DATE.format(new Date(`${c.session_date}T00:00:00Z`))}, period ${c.slot}`;
              return (
                <li key={c.id} className="grid gap-2 py-3 sm:grid-cols-[1fr_auto] sm:items-start">
                  <div className="text-sm">
                    <div className="font-medium">
                      {canDecide && `${c.student_name} (${c.roll_no}) · `}
                      {session}
                    </div>
                    <div className="text-muted-foreground">
                      {c.current_status} → {c.requested_status}: “{c.reason}”
                    </div>
                    {c.status !== "pending" && (
                      <div className="text-muted-foreground text-xs">
                        {c.status === "approved" ? "Approved" : "Rejected"} by {c.decided_by}
                        {c.decision_note && `: “${c.decision_note}”`}
                      </div>
                    )}
                  </div>
                  <div className="flex items-center gap-2 sm:justify-end">
                    {c.status === "pending" && canDecide ? (
                      <DecideCorrection id={c.id} summary={`${c.student_name}: ${session}, ${c.current_status} → ${c.requested_status}. “${c.reason}”`} />
                    ) : (
                      <Badge variant={c.status === "approved" ? "outline" : c.status === "rejected" ? "destructive" : "secondary"}>
                        {c.status}
                      </Badge>
                    )}
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}
