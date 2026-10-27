"use client";

import { useState, useTransition } from "react";
import { toast } from "sonner";

import { NativeSelect } from "@/components/native-select";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import type { Notice, Severity } from "@/lib/api";

import { deleteScheduledNotice, editNotice, withdrawNotice } from "./actions";

/** ISO to a datetime-local value in the browser's zone. */
function local(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  return new Date(d.getTime() - d.getTimezoneOffset() * 60_000).toISOString().slice(0, 16);
}

/** Corrects a notice's wording, severity or expiry; its audience stays. */
export function EditNotice({ notice }: { notice: Notice }) {
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState(notice.title);
  const [body, setBody] = useState(notice.body);
  const [severity, setSeverity] = useState<Severity>(notice.severity);
  const [expires, setExpires] = useState(local(notice.expires_at));
  const [pending, startTransition] = useTransition();

  function save(e: React.FormEvent) {
    e.preventDefault();
    startTransition(async () => {
      const result = await editNotice(notice.id, {
        title,
        body,
        severity,
        expires_at: expires ? new Date(expires).toISOString() : null,
      });
      if (result?.ok) {
        toast.success("Notice corrected");
        setOpen(false);
      } else if (result) toast.error(result.message);
    });
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="link" size="sm" className="h-auto p-0">
          Edit
        </Button>
      </DialogTrigger>
      <DialogContent className="max-h-[90svh] gap-6 overflow-y-auto p-6 sm:max-w-2xl sm:p-8">
        <DialogHeader>
          <DialogTitle>Correct this notice</DialogTitle>
          <DialogDescription>
            Readers see it marked edited; the log keeps the earlier text.
            {severity === "critical" && " Everyone acknowledges a corrected critical notice again."}
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={save} className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor={`edit-title-${notice.id}`}>Title</Label>
            <Input id={`edit-title-${notice.id}`} value={title} onChange={(e) => setTitle(e.target.value)} required maxLength={200} />
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`edit-body-${notice.id}`}>Message</Label>
            <Textarea id={`edit-body-${notice.id}`} value={body} onChange={(e) => setBody(e.target.value)} required rows={6} maxLength={10000} />
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="grid gap-2">
              <Label htmlFor={`edit-severity-${notice.id}`}>Severity</Label>
              <NativeSelect id={`edit-severity-${notice.id}`} value={severity} onChange={(e) => setSeverity(e.target.value as Severity)}>
                <option value="general">General</option>
                <option value="important">Important</option>
                <option value="critical">Critical</option>
              </NativeSelect>
            </div>
            <div className="grid gap-2">
              <Label htmlFor={`edit-expires-${notice.id}`}>Expires (optional)</Label>
              <Input id={`edit-expires-${notice.id}`} type="datetime-local" value={expires} onChange={(e) => setExpires(e.target.value)} />
            </div>
          </div>
          <DialogFooter>
            <Button type="submit" disabled={pending || !title.trim() || !body.trim()}>
              {pending ? "Saving…" : "Save correction"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Ends a published notice now, with a reason. */
export function WithdrawNotice({ notice }: { notice: Notice }) {
  const [open, setOpen] = useState(false);
  const [reason, setReason] = useState("");
  const [pending, startTransition] = useTransition();

  function withdraw(e: React.FormEvent) {
    e.preventDefault();
    startTransition(async () => {
      const result = await withdrawNotice(notice.id, reason);
      if (result?.ok) {
        toast.success("Notice withdrawn");
        setOpen(false);
      } else if (result) toast.error(result.message);
    });
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="link" size="sm" className="text-destructive h-auto p-0">
          Withdraw
        </Button>
      </DialogTrigger>
      <DialogContent className="gap-6 p-6 sm:max-w-lg sm:p-8">
        <DialogHeader>
          <DialogTitle>Withdraw “{notice.title}”?</DialogTitle>
          <DialogDescription>
            It disappears for its audience and from the public board at once. Emails already sent can&apos;t be recalled;
            publish a correction notice if people need to know.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={withdraw} className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor={`withdraw-${notice.id}`}>Reason</Label>
            <Input
              id={`withdraw-${notice.id}`}
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              placeholder="Sent to the wrong batch"
              required
              minLength={3}
              maxLength={500}
            />
          </div>
          <DialogFooter>
            <Button type="submit" variant="destructive" disabled={pending || reason.trim().length < 3}>
              {pending ? "Withdrawing…" : "Withdraw"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** A scheduled notice nobody has seen can simply be deleted. */
export function DeleteScheduledNotice({ notice }: { notice: Notice }) {
  const [pending, startTransition] = useTransition();
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button variant="link" size="sm" className="text-destructive h-auto p-0" disabled={pending}>
          Delete
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete this scheduled notice?</AlertDialogTitle>
          <AlertDialogDescription>“{notice.title}” hasn&apos;t been published yet, so nobody has seen it.</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction
            onClick={() =>
              startTransition(async () => {
                const result = await deleteScheduledNotice(notice.id);
                if (result?.ok) toast.success("Scheduled notice deleted");
                else if (result) toast.error(result.message);
              })
            }
          >
            Delete
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
