import { Info, OctagonAlert, TriangleAlert } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import type { PublicNotice, Severity } from "@/lib/api";
import { cn } from "@/lib/utils";

// Icon, label and colour together, so severity never depends on colour alone (§9.6).
const SEVERITY: Record<Severity, { label: string; icon: typeof Info; badge: string; edge: string }> = {
  general: { label: "General", icon: Info, badge: "", edge: "border-l-border" },
  important: {
    label: "Important",
    icon: TriangleAlert,
    badge: "border-amber-500/40 bg-amber-500/10 text-amber-800 dark:text-amber-300",
    edge: "border-l-amber-500",
  },
  critical: {
    label: "Critical",
    icon: OctagonAlert,
    badge: "border-destructive/40 bg-destructive/10 text-destructive",
    edge: "border-l-destructive",
  },
};

const DATE = new Intl.DateTimeFormat("en-IN", { dateStyle: "medium", timeZone: "Asia/Kolkata" });

export function SeverityBadge({ severity }: { severity: Severity }) {
  const { label, icon: Icon, badge } = SEVERITY[severity];
  return (
    <Badge variant="outline" className={cn("gap-1", badge)}>
      <Icon className="size-3" aria-hidden />
      {label}
    </Badge>
  );
}

export function NoticeCard({
  notice,
  audience,
  unread = false,
  compact = false,
  footer,
}: {
  notice: PublicNotice;
  /** e.g. a program name; omitted for institution-wide notices. */
  audience?: string | null;
  unread?: boolean;
  /** Title and meta only, for dashboard lists. */
  compact?: boolean;
  /** Attachments, receipts and the like. */
  footer?: React.ReactNode;
}) {
  return (
    <article className={cn("bg-card rounded-lg border border-l-4 p-4", SEVERITY[notice.severity].edge)}>
      <div className="text-muted-foreground mb-1 flex flex-wrap items-center gap-2 text-xs">
        <SeverityBadge severity={notice.severity} />
        <time dateTime={notice.published_at}>{DATE.format(new Date(notice.published_at))}</time>
        {audience && <span>· {audience}</span>}
        {unread && (
          <Badge className="ml-auto" aria-label="Unread">
            New
          </Badge>
        )}
      </div>
      <h2 className={cn("font-semibold", !compact && "text-lg")}>{notice.title}</h2>
      {!compact && <p className="text-muted-foreground mt-1 whitespace-pre-line">{notice.body}</p>}
      {footer && <div className="mt-3 flex flex-wrap items-center gap-3 text-sm">{footer}</div>}
    </article>
  );
}
