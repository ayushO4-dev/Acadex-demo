import { Paperclip } from "lucide-react";
import Link from "next/link";

import { ApiProblem } from "@/components/api-problem";
import { NoticeCard } from "@/components/notice-card";
import { PageHeader } from "@/components/page-header";
import { asUser, getMe, getNotices } from "@/lib/acadex";
import type { NoticeList } from "@/lib/api";
import { cn } from "@/lib/utils";

import { MarkReadButton } from "./mark-read-button";
import { EmailPreference, ReceiptsButton } from "./notice-controls";
import { AddNoticeDialog, type Program } from "./notice-form";
import { DeleteScheduledNotice, EditNotice, WithdrawNotice } from "./notice-manage";

export const dynamic = "force-dynamic";

export default async function NoticesPage({ searchParams }: { searchParams: Promise<{ before?: string; before_id?: string }> }) {
  const params = await searchParams;
  const before = params.before && params.before_id ? { at: params.before, id: params.before_id } : undefined;
  const me = await getMe();
  const canPublish = me.permissions.includes("notices:publish");

  let list: NoticeList;
  let programs: Program[] = [];
  let emailLevel: "all" | "important" | "critical";
  try {
    [list, programs, { email_level: emailLevel }] = await Promise.all([
      getNotices(before),
      canPublish ? asUser<Program[]>("/programs") : Promise.resolve([]),
      asUser<{ email_level: "all" | "important" | "critical" }>("/notices/preferences"),
    ]);
  } catch (err) {
    return (
      <>
        <PageHeader title="Notices" />
        <ApiProblem error={err} />
      </>
    );
  }

  const unreadIds = list.notices.filter((n) => !n.read).map((n) => n.id);
  const now = Date.now();

  return (
    <>
      <PageHeader
        title="Notices"
        description={
          canPublish
            ? "Every notice, including scheduled and expired ones."
            : "Notices for the institute and your program, newest first."
        }
        actions={canPublish && <AddNoticeDialog programs={programs} />}
      />

      <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
        <p className="text-muted-foreground text-sm">
          {list.unread === 0 ? "You're all caught up." : `${list.unread} unread`}
        </p>
        <div className="flex flex-wrap items-center gap-3">
          <EmailPreference level={emailLevel} />
          {unreadIds.length > 0 && <MarkReadButton ids={unreadIds} />}
        </div>
      </div>

      {list.notices.length === 0 ? (
        <p className="text-muted-foreground">No notices yet.</p>
      ) : (
        <div className="grid gap-3">
          {list.notices.map((notice) => {
            const scheduled = new Date(notice.published_at).getTime() > now;
            const withdrawn = notice.withdrawn_at !== null;
            const expired = !withdrawn && notice.expires_at !== null && new Date(notice.expires_at).getTime() <= now;
            return (
              <div key={notice.id} id={`notice-${notice.id}`} className={cn("scroll-mt-6", (expired || scheduled || withdrawn) && "opacity-60")}>
                <NoticeCard
                  notice={notice}
                  audience={[
                    notice.individual ? "named recipients" : notice.program_name,
                    notice.batch_year && `batch ${notice.batch_year}`,
                    notice.section && `section ${notice.section}`,
                    canPublish && notice.author && `by ${notice.author}`,
                    scheduled && `scheduled for ${new Date(notice.published_at).toLocaleString("en-IN", { dateStyle: "medium", timeStyle: "short" })}`,
                    expired && "expired",
                    withdrawn && `withdrawn: ${notice.withdraw_reason}`,
                    notice.edited_at && "edited",
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                  unread={!notice.read && !expired && !scheduled && !withdrawn}
                  footer={
                    (notice.attachments.length > 0 || canPublish) && (
                      <>
                        {notice.attachments.map((a) => (
                          <a key={a.id} href={`/notices/files/${notice.id}/${a.id}`} target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1 underline-offset-4 hover:underline">
                            <Paperclip className="size-3" aria-hidden />
                            {a.name}
                          </a>
                        ))}
                        {canPublish && !scheduled && <ReceiptsButton id={notice.id} critical={notice.severity === "critical"} />}
                        {canPublish && !withdrawn && <EditNotice notice={notice} />}
                        {canPublish && !withdrawn && !scheduled && <WithdrawNotice notice={notice} />}
                        {canPublish && scheduled && <DeleteScheduledNotice notice={notice} />}
                      </>
                    )
                  }
                />
              </div>
            );
          })}
        </div>
      )}
      {(before || list.next) && (
        <div className="mt-4 flex justify-between text-sm">
          {before ? (
            <Link href="/notices" className="underline-offset-4 hover:underline">
              Newest notices
            </Link>
          ) : (
            <span />
          )}
          {list.next && (
            <Link
              href={`/notices?${new URLSearchParams({ before: list.next[0], before_id: list.next[1] })}`}
              className="underline-offset-4 hover:underline"
            >
              Older notices
            </Link>
          )}
        </div>
      )}
    </>
  );
}
