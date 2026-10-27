"use client";

import { Bell } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { toast } from "sonner";

import { SeverityBadge } from "@/components/notice-card";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import type { Notice } from "@/lib/api";
import { cn } from "@/lib/utils";

import { markRead, recentNotices } from "@/app/(app)/notices/actions";

const WHEN = new Intl.DateTimeFormat("en-IN", { day: "numeric", month: "short", timeZone: "Asia/Kolkata" });

/**
 * The bell (architecture.md §8.2): a popover with All and Unread tabs that
 * marks what it shows as read, kept current over server-sent events. A new
 * notice also pops up a toast; a critical one refreshes the page, which
 * brings up its acknowledgement.
 */
export function NotificationsBell({ initialUnread }: { initialUnread: number }) {
  const [unread, setUnread] = useState(initialUnread);
  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState<"all" | "unread">("unread");
  const [notices, setNotices] = useState<Notice[] | null>(null);
  const router = useRouter();

  // Live updates; the browser reconnects by itself if the stream drops.
  useEffect(() => {
    const events = new EventSource("/notices/stream");
    events.addEventListener("unread", (e) => setUnread(JSON.parse((e as MessageEvent).data).unread));
    events.addEventListener("notice", (e) => {
      const n = JSON.parse((e as MessageEvent).data) as { title: string; severity: string; unread: number };
      setUnread(n.unread);
      setNotices(null);
      if (n.severity === "critical") router.refresh();
      else
        toast(n.title, {
          description: n.severity === "important" ? "Important notice" : "New notice",
          action: { label: "Open", onClick: () => router.push("/notices") },
        });
    });
    return () => events.close();
  }, [router]);

  async function onOpenChange(next: boolean) {
    setOpen(next);
    if (!next) return;
    const list = await recentNotices();
    if (!list) return;
    setNotices(list.notices);
    // Opening it reads what's showing.
    const fresh = list.notices.filter((n) => !n.read).map((n) => n.id);
    if (fresh.length > 0) {
      setUnread(Math.max(0, list.unread - fresh.length));
      await markRead(fresh);
    }
  }

  const shown = (notices ?? []).filter((n) => tab === "all" || !n.read);
  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="icon" className="relative" aria-label={unread > 0 ? `Notices, ${unread} unread` : "Notices"}>
          <Bell />
          {unread > 0 && (
            <span className="bg-destructive absolute top-1 right-1 grid h-4 min-w-4 place-items-center rounded-full px-1 text-[10px] leading-none font-semibold text-white tabular-nums">
              {unread > 9 ? "9+" : unread}
            </span>
          )}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-80 p-0">
        <div className="flex items-center justify-between border-b px-3 py-2">
          <span className="text-sm font-medium">Notices</span>
          <div role="tablist" className="bg-muted flex rounded-md p-0.5 text-xs">
            {(["unread", "all"] as const).map((t) => (
              <button
                key={t}
                role="tab"
                aria-selected={tab === t}
                onClick={() => setTab(t)}
                className={cn("rounded px-2 py-0.5 capitalize", tab === t ? "bg-background shadow-xs" : "text-muted-foreground")}
              >
                {t}
              </button>
            ))}
          </div>
        </div>
        <ul className="max-h-80 divide-y overflow-y-auto">
          {notices === null && <li className="text-muted-foreground p-3 text-sm">Loading…</li>}
          {notices !== null && shown.length === 0 && (
            <li className="text-muted-foreground p-3 text-sm">{tab === "unread" ? "You're all caught up." : "No notices."}</li>
          )}
          {shown.map((n) => (
            <li key={n.id} className="px-3 py-2">
              <div className="flex items-center gap-2">
                <SeverityBadge severity={n.severity} />
                <span className="text-muted-foreground text-xs">{WHEN.format(new Date(n.published_at))}</span>
                {!n.read && <span className="bg-primary ml-auto size-2 rounded-full" aria-label="Unread" />}
              </div>
              <div className="mt-1 line-clamp-2 text-sm font-medium">{n.title}</div>
            </li>
          ))}
        </ul>
        <div className="border-t p-2">
          <Button asChild variant="ghost" size="sm" className="w-full" onClick={() => setOpen(false)}>
            <Link href="/notices">View all</Link>
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
