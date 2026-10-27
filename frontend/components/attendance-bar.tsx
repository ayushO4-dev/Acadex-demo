import { attendanceTone } from "@/components/attendance-ring";
import { cn } from "@/lib/utils";

const FILL = { ok: "bg-emerald-500", warning: "bg-amber-500", danger: "bg-red-500" } as const;

/**
 * Attendance as a bar, with the shortage threshold marked. The percentage is
 * also given as text, so the bar is never the only signal.
 */
export function AttendanceBar({
  percentage,
  threshold,
  shortage,
}: {
  percentage: number;
  threshold: number;
  shortage: boolean;
}) {
  return (
    <div
      role="meter"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percentage}
      aria-label={`${percentage.toFixed(1)}% attended, threshold ${threshold}%`}
      className="bg-muted relative h-2 w-full overflow-visible rounded-full"
    >
      <div
        className={cn("h-full rounded-full", FILL[attendanceTone(percentage, threshold, shortage)])}
        style={{ width: `${Math.min(percentage, 100)}%` }}
      />
      <div
        aria-hidden
        className="bg-foreground absolute -top-1 h-4 w-0.5 rounded-full"
        style={{ left: `${threshold}%` }}
      />
    </div>
  );
}
