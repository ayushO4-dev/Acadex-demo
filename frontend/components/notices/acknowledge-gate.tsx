"use client";

import { OctagonAlert } from "lucide-react";
import { useRouter } from "next/navigation";
import { useTransition } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import type { Notice } from "@/lib/api";

import { acknowledge } from "@/app/(app)/notices/actions";

/**
 * A critical notice must be acknowledged before anything else (§9.6): a
 * dialog that can't be dismissed, one notice at a time, oldest first.
 */
export function AcknowledgeGate({ notices }: { notices: Notice[] }) {
  const [pending, startTransition] = useTransition();
  const router = useRouter();
  const notice = notices[0];
  if (!notice) return null;
  return (
    <Dialog open>
      <DialogContent
        showCloseButton={false}
        onEscapeKeyDown={(e) => e.preventDefault()}
        onPointerDownOutside={(e) => e.preventDefault()}
        onInteractOutside={(e) => e.preventDefault()}
        className="sm:max-w-lg"
      >
        <DialogHeader>
          <DialogTitle className="text-destructive flex items-center gap-2">
            <OctagonAlert className="size-5" aria-hidden />
            {notice.title}
          </DialogTitle>
          <DialogDescription>
            Critical notice{notice.author ? ` from ${notice.author}` : ""}
            {notices.length > 1 && ` · 1 of ${notices.length}`}
          </DialogDescription>
        </DialogHeader>
        <p className="max-h-[50vh] overflow-y-auto text-sm whitespace-pre-line">{notice.body}</p>
        {notice.attachments.length > 0 && (
          <ul className="text-sm">
            {notice.attachments.map((a) => (
              <li key={a.id}>
                <a href={`/notices/files/${notice.id}/${a.id}`} target="_blank" rel="noopener noreferrer" className="underline underline-offset-4">
                  {a.name}
                </a>
              </li>
            ))}
          </ul>
        )}
        <DialogFooter>
          <Button
            disabled={pending}
            onClick={() =>
              startTransition(async () => {
                const result = await acknowledge(notice.id);
                if (result && !result.ok) toast.error(result.message);
                else router.refresh();
              })
            }
          >
            {pending ? "Saving…" : "I have read and understood this"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
