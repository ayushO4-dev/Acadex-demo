"use client";

import { useState, useTransition } from "react";
import { toast } from "sonner";

import { NativeSelect } from "@/components/native-select";
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
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import type { AttendanceStatusName } from "@/lib/api";

import { decideCorrection, requestCorrection } from "./correction-actions";

const LABEL: Record<AttendanceStatusName, string> = { present: "Present", absent: "Absent", late: "Late", excused: "Excused" };

/** A student disputes one session's mark; their HOD decides. */
export function RequestCorrection({
  session,
}: {
  session: { offering_id: string; date: string; slot: number; status: AttendanceStatusName };
}) {
  const [open, setOpen] = useState(false);
  const options = (Object.keys(LABEL) as AttendanceStatusName[]).filter((s) => s !== session.status);
  const [status, setStatus] = useState<AttendanceStatusName>(options.includes("present") ? "present" : options[0]);
  const [reason, setReason] = useState("");
  const [pending, startTransition] = useTransition();

  function submit() {
    startTransition(async () => {
      const result = await requestCorrection({
        offering_id: session.offering_id,
        session_date: session.date,
        slot: session.slot,
        requested_status: status,
        reason,
      });
      if (result.ok) {
        toast.success("Sent to your HOD");
        setOpen(false);
      } else toast.error(result.message);
    });
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="outline" size="sm">
          Request correction
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Request a correction</DialogTitle>
          <DialogDescription>
            {session.date}, period {session.slot}: marked {LABEL[session.status].toLowerCase()}. Your HOD reviews the request.
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor="requested">Should be</Label>
            <NativeSelect id="requested" value={status} onChange={(e) => setStatus(e.target.value as AttendanceStatusName)}>
              {options.map((s) => (
                <option key={s} value={s}>
                  {LABEL[s]}
                </option>
              ))}
            </NativeSelect>
          </div>
          <div className="grid gap-2">
            <Label htmlFor="reason">Reason</Label>
            <Textarea
              id="reason"
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              minLength={5}
              maxLength={1000}
              rows={4}
              placeholder="e.g. Represented the college at a technical quiz; letter from the organisers attached with the department."
            />
          </div>
        </div>
        <DialogFooter>
          <Button onClick={submit} disabled={pending || reason.trim().length < 5}>
            {pending ? "Sending…" : "Send request"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function DecideCorrection({ id, summary }: { id: string; summary: string }) {
  const [open, setOpen] = useState(false);
  const [note, setNote] = useState("");
  const [pending, startTransition] = useTransition();

  function decide(approve: boolean) {
    startTransition(async () => {
      const result = await decideCorrection(id, approve, note);
      if (result.ok) {
        toast.success(approve ? "Approved: the mark is corrected" : "Rejected");
        setOpen(false);
      } else toast.error(result.message);
    });
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button size="sm">Review</Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Review correction</DialogTitle>
          <DialogDescription>{summary}</DialogDescription>
        </DialogHeader>
        <div className="grid gap-2">
          <Label htmlFor={`note-${id}`}>Note to the student (optional)</Label>
          <Textarea id={`note-${id}`} value={note} onChange={(e) => setNote(e.target.value)} maxLength={1000} rows={3} />
        </div>
        <DialogFooter className="gap-2">
          <Button variant="outline" onClick={() => decide(false)} disabled={pending}>
            Reject
          </Button>
          <Button onClick={() => decide(true)} disabled={pending}>
            Approve and correct the mark
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
