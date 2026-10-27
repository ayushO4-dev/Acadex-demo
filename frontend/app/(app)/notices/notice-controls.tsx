"use client";

import { useState, useTransition } from "react";
import { toast } from "sonner";

import { NativeSelect } from "@/components/native-select";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Progress } from "@/components/ui/progress";
import type { NoticeReceipts } from "@/lib/api";

import { receipts, setEmailLevel } from "./actions";

const WHEN = new Intl.DateTimeFormat("en-IN", { day: "numeric", month: "short", hour: "numeric", minute: "2-digit", timeZone: "Asia/Kolkata" });

/** Publishers: who it reached, read and (critical) acknowledged. */
export function ReceiptsButton({ id, critical }: { id: string; critical: boolean }) {
  const [data, setData] = useState<NoticeReceipts | null>(null);
  const [pending, startTransition] = useTransition();
  return (
    <Dialog onOpenChange={(open) => open && startTransition(async () => setData(await receipts(id)))}>
      <DialogTrigger asChild>
        <Button variant="link" size="sm" className="h-auto p-0">
          Read receipts
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Read receipts</DialogTitle>
          <DialogDescription>Everyone this notice reaches, and who has read{critical ? " and acknowledged" : ""} it.</DialogDescription>
        </DialogHeader>
        {pending || !data ? (
          <p className="text-muted-foreground text-sm">{pending ? "Loading…" : "Couldn't load the receipts."}</p>
        ) : (
          <div className="grid gap-4">
            <div className="grid gap-1 text-sm">
              <div className="flex justify-between">
                <span>
                  Read by {data.read} of {data.audience}
                </span>
                <span className="text-muted-foreground">{data.emailed} emailed</span>
              </div>
              <Progress value={data.audience ? (100 * data.read) / data.audience : 0} />
              {critical && (
                <span className="text-muted-foreground">
                  Acknowledged by {data.acknowledged} of {data.audience}
                </span>
              )}
            </div>
            <ul className="max-h-80 divide-y overflow-y-auto rounded-md border text-sm">
              {data.readers.map((r, i) => (
                <li key={i} className="flex items-center justify-between gap-2 px-3 py-1.5">
                  <span className="truncate">
                    {r.display_name}
                    {r.roll_no && <span className="text-muted-foreground font-mono text-xs"> {r.roll_no}</span>}
                  </span>
                  <span className="text-muted-foreground shrink-0 text-xs">
                    {r.acknowledged_at ? `Acknowledged ${WHEN.format(new Date(r.acknowledged_at))}` : r.read_at ? `Read ${WHEN.format(new Date(r.read_at))}` : "Not read"}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

/** Which notices reach the user by email; critical ones always do. */
export function EmailPreference({ level }: { level: "all" | "important" | "critical" }) {
  const [value, setValue] = useState(level);
  const [pending, startTransition] = useTransition();
  return (
    <label className="text-muted-foreground flex items-center gap-2 text-sm">
      Email me
      <NativeSelect
        value={value}
        disabled={pending}
        className="w-auto"
        onChange={(e) => {
          const next = e.target.value as typeof value;
          setValue(next);
          startTransition(async () => {
            const result = await setEmailLevel(next);
            if (result && !result.ok) toast.error(result.message);
            else toast.success("Email preference saved");
          });
        }}
      >
        <option value="all">every notice</option>
        <option value="important">important and critical notices</option>
        <option value="critical">critical notices only</option>
      </NativeSelect>
    </label>
  );
}
