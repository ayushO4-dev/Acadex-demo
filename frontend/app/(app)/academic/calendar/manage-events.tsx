"use client";

import { Pencil, Plus, Trash2 } from "lucide-react";
import { useActionState, useEffect, useState, useTransition } from "react";
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
import { EVENT_KIND } from "@/components/event-kind";
import type { CalendarEvent, EventKind } from "@/lib/api";

import { type Outcome, removeEvent, saveEvent } from "./actions";

/** Adds an event, or (given one) changes it. */
export function EventDialog({ programs, event }: { programs: { id: number; name: string }[]; event?: CalendarEvent }) {
  const [open, setOpen] = useState(false);
  const [result, action, pending] = useActionState<Outcome, FormData>(saveEvent.bind(null, event?.id ?? null), null);
  const key = event?.id ?? "new";

  useEffect(() => {
    if (result?.ok) {
      toast.success(event ? "Event changed" : "Event added");
      setOpen(false);
    } else if (result) toast.error(result.message);
  }, [result, event]);

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        {event ? (
          <Button variant="ghost" size="icon" aria-label={`Change ${event.title}`}>
            <Pencil />
          </Button>
        ) : (
          <Button>
            <Plus />
            Add event
          </Button>
        )}
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{event ? `Change “${event.title}”` : "Add to the academic calendar"}</DialogTitle>
          <DialogDescription>
            Everyone it concerns sees it at once, and in their calendar export. Holidays and class suspensions stop
            attendance being marked those days.
          </DialogDescription>
        </DialogHeader>
        <form action={action} className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor={`title-${key}`}>Title</Label>
            <Input id={`title-${key}`} name="title" required maxLength={200} defaultValue={event?.title} />
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="grid gap-2">
              <Label htmlFor={`kind-${key}`}>Type</Label>
              <NativeSelect id={`kind-${key}`} name="kind" defaultValue={event?.kind ?? "event"}>
                {(Object.keys(EVENT_KIND) as EventKind[]).map((k) => (
                  <option key={k} value={k}>
                    {EVENT_KIND[k].label}
                  </option>
                ))}
              </NativeSelect>
            </div>
            <div className="grid gap-2">
              <Label htmlFor={`program-${key}`}>For</Label>
              <NativeSelect id={`program-${key}`} name="program_id" defaultValue={event?.program_id ?? ""}>
                <option value="">Everyone</option>
                {programs.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </NativeSelect>
            </div>
            <div className="grid gap-2">
              <Label htmlFor={`starts-${key}`}>From</Label>
              <Input id={`starts-${key}`} name="starts_on" type="date" required defaultValue={event?.starts_on} />
            </div>
            <div className="grid gap-2">
              <Label htmlFor={`ends-${key}`}>To (optional)</Label>
              <Input id={`ends-${key}`} name="ends_on" type="date" defaultValue={event?.ends_on} />
            </div>
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`details-${key}`}>Details (optional)</Label>
            <Textarea id={`details-${key}`} name="description" maxLength={2000} rows={3} defaultValue={event?.description ?? ""} />
          </div>
          <DialogFooter>
            <Button type="submit" disabled={pending}>
              {pending ? "Saving…" : event ? "Save" : "Add event"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function RemoveEventButton({ id, title }: { id: string; title: string }) {
  const [pending, startTransition] = useTransition();
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button variant="ghost" size="icon" aria-label={`Remove ${title}`} disabled={pending}>
          <Trash2 />
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Remove “{title}”?</AlertDialogTitle>
          <AlertDialogDescription>It disappears from everyone&apos;s calendar.</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction
            onClick={() =>
              startTransition(async () => {
                const result = await removeEvent(id);
                if (result?.ok) toast.success("Removed");
                else if (result) toast.error(result.message);
              })
            }
          >
            Remove
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
