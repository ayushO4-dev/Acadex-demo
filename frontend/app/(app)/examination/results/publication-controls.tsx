"use client";

import { Plus } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
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
import type { Publication, ResultKind } from "@/lib/api";

import { KIND_LABEL } from "./labels";
import {
  type ActionResult,
  approveWithdrawal,
  cancelWithdrawal,
  createResultSet,
  publishResults,
  requestPublishing,
  requestWithdrawal,
  withdrawRequest,
} from "./actions";


function report(result: ActionResult, success: string): void {
  if (result.ok) toast.success(success);
  else toast.error(result.message);
}

/** Opens an empty result set, then goes straight to its marks entry. */
export function NewSetDialog({ programs }: { programs: { id: number; name: string }[] }) {
  const [open, setOpen] = useState(false);
  const [pending, startTransition] = useTransition();
  const router = useRouter();
  const year = new Date().getMonth() >= 6 ? new Date().getFullYear() : new Date().getFullYear() - 1;

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button>
          <Plus />
          New result set
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>New result set</DialogTitle>
          <DialogDescription>
            One program&apos;s results for one semester. Backlog sets hold the students who registered that
            semester&apos;s subjects on a backlog form that year.
          </DialogDescription>
        </DialogHeader>
        <form
          className="grid gap-4"
          action={(form) =>
            startTransition(async () => {
              const result = await createResultSet({
                program_id: Number(form.get("program_id")),
                semester: Number(form.get("semester")),
                academic_year: Number(form.get("academic_year")),
                kind: String(form.get("kind")) as ResultKind,
              });
              report(result, "Result set created");
              if (result.ok && result.id) {
                setOpen(false);
                router.push(`/examination/results/${result.id}`);
              }
            })
          }
        >
          <div className="grid gap-2">
            <Label htmlFor="program_id">Program</Label>
            <NativeSelect id="program_id" name="program_id" required>
              {programs.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </NativeSelect>
          </div>
          <div className="grid gap-4 sm:grid-cols-3">
            <div className="grid gap-2">
              <Label htmlFor="semester">Semester</Label>
              <Input id="semester" name="semester" type="number" min={1} max={8} required defaultValue={5} />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="academic_year">Academic year (start)</Label>
              <Input id="academic_year" name="academic_year" type="number" required defaultValue={year} />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="kind">Kind</Label>
              <NativeSelect id="kind" name="kind" defaultValue="regular">
                <option value="regular">Regular</option>
                <option value="backlog">Backlog</option>
              </NativeSelect>
            </div>
          </div>
          <DialogFooter>
            <Button type="submit" disabled={pending}>
              {pending ? "Creating…" : "Create and enter marks"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/**
 * Two people publish (architecture.md §9.3): one asks, a different one in the
 * exam cell approves. The requester can withdraw to correct a mark.
 */
export function PublicationActions({ publication, meId }: { publication: Publication; meId: string }) {
  const [pending, startTransition] = useTransition();
  const what = `${publication.program_name}, semester ${publication.semester} (${KIND_LABEL[publication.kind].toLowerCase()})`;
  if (publication.published_at) return <WithdrawalActions publication={publication} meId={meId} what={what} />;

  const marks = (
    <Button asChild size="sm" variant="outline">
      <Link href={`/examination/results/${publication.id}`}>{publication.requested_at || publication.kind === "revaluation" ? "Review marks" : "Enter marks"}</Link>
    </Button>
  );

  if (!publication.requested_by_id) {
    return (
      <div className="flex justify-end gap-2">
        {marks}
        <Button
          size="sm"
          disabled={pending || publication.students === 0}
          title={publication.students === 0 ? "Enter marks first" : undefined}
          onClick={() =>
            startTransition(async () =>
              report(await requestPublishing(publication.id), "Requested. Someone else in the exam cell must approve."),
            )
          }
        >
          {pending ? "Requesting…" : "Request publishing"}
        </Button>
      </div>
    );
  }

  if (publication.requested_by_id === meId) {
    return (
      <div className="flex justify-end gap-2">
        {marks}
        <Button
          size="sm"
          variant="outline"
          disabled={pending}
          onClick={() => startTransition(async () => report(await withdrawRequest(publication.id), "Request withdrawn"))}
        >
          Withdraw request
        </Button>
      </div>
    );
  }

  return (
    <div className="flex justify-end gap-2">
      {marks}
      <AlertDialog>
        <AlertDialogTrigger asChild>
          <Button size="sm" disabled={pending}>
            {pending ? "Publishing…" : "Approve and publish"}
          </Button>
        </AlertDialogTrigger>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Publish results for {what}?</AlertDialogTitle>
            <AlertDialogDescription>
              {publication.requested_by} asked to publish these. {publication.students} students will see their results
              immediately and get a notice. Publishing can&apos;t be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={() =>
                startTransition(async () => report(await publishResults(publication.id), `Published results for ${what}`))
              }
            >
              Approve and publish
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

/**
 * Published results found to be wrong: one exam-cell member asks to withdraw
 * them (saying why), a different one approves.
 */
function WithdrawalActions({ publication, meId, what }: { publication: Publication; meId: string; what: string }) {
  const [open, setOpen] = useState(false);
  const [reason, setReason] = useState("");
  const [pending, startTransition] = useTransition();

  if (!publication.withdrawal_requested_by_id) {
    return (
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogTrigger asChild>
          <Button size="sm" variant="outline">
            Withdraw…
          </Button>
        </DialogTrigger>
        <DialogContent className="gap-6 p-6 sm:max-w-lg sm:p-8">
          <DialogHeader>
            <DialogTitle>Withdraw results for {what}?</DialogTitle>
            <DialogDescription>
              Someone else in the exam cell approves. Then students stop seeing these results, their grade cards come down,
              and the marks can be corrected and published again. Students are told.
            </DialogDescription>
          </DialogHeader>
          <form
            className="grid gap-4"
            onSubmit={(e) => {
              e.preventDefault();
              startTransition(async () => {
                const result = await requestWithdrawal(publication.id, reason);
                report(result, "Withdrawal requested. Someone else in the exam cell must approve.");
                if (result.ok) setOpen(false);
              });
            }}
          >
            <div className="grid gap-2">
              <Label htmlFor={`withdraw-${publication.id}`}>What&apos;s wrong</Label>
              <Textarea
                id={`withdraw-${publication.id}`}
                value={reason}
                onChange={(e) => setReason(e.target.value)}
                placeholder="EC301 external marks were entered for the wrong paper"
                rows={3}
                minLength={10}
                maxLength={500}
                required
              />
            </div>
            <DialogFooter>
              <Button type="submit" variant="destructive" disabled={pending || reason.trim().length < 10}>
                {pending ? "Requesting…" : "Request withdrawal"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    );
  }

  if (publication.withdrawal_requested_by_id === meId) {
    return (
      <Button
        size="sm"
        variant="outline"
        disabled={pending}
        onClick={() => startTransition(async () => report(await cancelWithdrawal(publication.id), "Withdrawal request taken back"))}
      >
        Cancel withdrawal
      </Button>
    );
  }

  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button size="sm" variant="destructive" disabled={pending}>
          {pending ? "Withdrawing…" : "Approve withdrawal"}
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Withdraw results for {what}?</AlertDialogTitle>
          <AlertDialogDescription>
            {publication.withdrawal_requested_by} asked: “{publication.withdrawal_reason}”. {publication.students} students
            stop seeing these results and their grade cards come down until the set is published again.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction
            onClick={() => startTransition(async () => report(await approveWithdrawal(publication.id), "Results withdrawn"))}
          >
            Withdraw results
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
