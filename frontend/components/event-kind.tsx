import { Ban, CalendarClock, FilePen, PartyPopper, Sparkles } from "lucide-react";

import type { EventKind } from "@/lib/api";
import { cn } from "@/lib/utils";

// Icon and label as well as colour, so the kind never depends on colour alone.
export const EVENT_KIND: Record<EventKind, { label: string; icon: typeof Ban; chip: string }> = {
  holiday: { label: "Holiday", icon: PartyPopper, chip: "bg-emerald-500/15 text-emerald-800 dark:text-emerald-300 border-emerald-500/30" },
  exam: { label: "Exam", icon: FilePen, chip: "bg-destructive/10 text-destructive border-destructive/30" },
  deadline: { label: "Deadline", icon: CalendarClock, chip: "bg-amber-500/15 text-amber-800 dark:text-amber-300 border-amber-500/30" },
  event: { label: "Event", icon: Sparkles, chip: "bg-sky-500/15 text-sky-800 dark:text-sky-300 border-sky-500/30" },
  class_suspension: { label: "Classes suspended", icon: Ban, chip: "bg-muted text-muted-foreground border-border" },
};

export function EventChip({ kind, title }: { kind: EventKind; title: string }) {
  const { icon: Icon, chip, label } = EVENT_KIND[kind];
  return (
    <span className={cn("flex items-center gap-1 truncate rounded border px-1.5 py-0.5 text-xs", chip)} title={`${label}: ${title}`}>
      <Icon className="size-3 shrink-0" aria-hidden />
      <span className="sr-only">{label}: </span>
      <span className="truncate">{title}</span>
    </span>
  );
}
