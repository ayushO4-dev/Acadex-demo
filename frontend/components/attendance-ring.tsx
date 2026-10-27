import { cn } from "@/lib/utils";

/** How close to the threshold: shortage, within 5 points of it, or comfortable. */
export function attendanceTone(percentage: number, threshold: number, shortage: boolean) {
  if (shortage) return "danger" as const;
  return percentage < threshold + 5 ? ("warning" as const) : ("ok" as const);
}

const ARC = {
  ok: "stroke-emerald-500",
  warning: "stroke-amber-500",
  danger: "stroke-red-500",
} as const;

/**
 * Overall attendance as a ring: the arc is coloured by how close it is to the
 * threshold, which is marked and labelled on the track. The percentage is in
 * the middle and in the label, so colour is never the only signal.
 */
export function AttendanceRing({
  percentage,
  threshold,
  shortage,
  size = 168,
}: {
  percentage: number;
  threshold: number;
  shortage: boolean;
  size?: number;
}) {
  const stroke = 14;
  const pad = 32; // room for the threshold label outside the track
  const box = size + pad * 2;
  const centre = box / 2;
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const filled = (Math.min(Math.max(percentage, 0), 100) / 100) * c;
  const angle = (threshold / 100) * 2 * Math.PI - Math.PI / 2;
  const at = (d: number) => [centre + Math.cos(angle) * d, centre + Math.sin(angle) * d] as const;
  const [x1, y1] = at(r - stroke / 2 - 3);
  const [x2, y2] = at(r + stroke / 2 + 3);
  const [lx, ly] = at(r + stroke / 2 + 7);
  // Anchor the label on the side away from the ring, whatever the threshold.
  const cos = Math.cos(angle);
  const anchor = cos < -0.3 ? "end" : cos > 0.3 ? "start" : "middle";
  const tone = attendanceTone(percentage, threshold, shortage);

  return (
    <svg
      width={box}
      height={box}
      viewBox={`0 0 ${box} ${box}`}
      role="img"
      aria-label={`${percentage.toFixed(1)}% attended; the threshold is ${threshold}%`}
      className="shrink-0"
    >
      <circle cx={centre} cy={centre} r={r} fill="none" strokeWidth={stroke} className="stroke-muted" />
      {percentage > 0 && (
        <circle
          cx={centre}
          cy={centre}
          r={r}
          fill="none"
          strokeWidth={stroke}
          strokeLinecap="round"
          strokeDasharray={`${filled} ${c}`}
          transform={`rotate(-90 ${centre} ${centre})`}
          className={cn(ARC[tone], "transition-[stroke-dasharray] duration-500")}
        />
      )}
      {/* The threshold: a notch across the track, outlined so it shows on any arc colour. */}
      <line x1={x1} y1={y1} x2={x2} y2={y2} strokeWidth={5} strokeLinecap="round" className="stroke-card" />
      <line x1={x1} y1={y1} x2={x2} y2={y2} strokeWidth={2} strokeLinecap="round" className="stroke-foreground" />
      <text x={lx} y={ly} dominantBaseline="central" textAnchor={anchor} className="fill-muted-foreground text-[11px] tabular-nums">
        {threshold}%
      </text>
      <text x={centre} y={centre - 6} dominantBaseline="central" textAnchor="middle" className={cn("fill-foreground font-semibold tabular-nums", size < 150 ? "text-2xl" : "text-3xl")}>
        {percentage.toFixed(1)}%
      </text>
      <text x={centre} y={centre + (size < 150 ? 16 : 20)} dominantBaseline="central" textAnchor="middle" className="fill-muted-foreground text-xs">
        attended
      </text>
    </svg>
  );
}
