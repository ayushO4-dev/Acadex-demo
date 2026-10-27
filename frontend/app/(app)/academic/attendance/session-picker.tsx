"use client";

import { usePathname, useRouter } from "next/navigation";

import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { Offering, SessionKind } from "@/lib/api";

const PERIODS = [1, 2, 3, 4, 5, 6, 7, 8];

/** Which session to mark: offering, date and period, kept in the URL so it's shareable. */
export function SessionPicker({
  offerings,
  offeringId,
  date,
  slot,
  kind,
  today,
}: {
  offerings: Offering[];
  offeringId: string;
  date: string;
  slot: number;
  kind: SessionKind;
  today: string;
}) {
  const offering = offerings.find((o) => o.id === offeringId);
  const choose = offering?.has_practical && offering.kind !== "lab";
  const router = useRouter();
  const pathname = usePathname();

  function go(next: { offering?: string; date?: string; slot?: number; kind?: SessionKind }) {
    const query = new URLSearchParams({
      offering: next.offering ?? offeringId,
      date: next.date ?? date,
      slot: String(next.slot ?? slot),
      kind: next.kind ?? kind,
    });
    router.push(`${pathname}?${query}`);
  }

  return (
    <div className={choose ? "grid gap-3 sm:grid-cols-[1fr_auto_auto_auto]" : "grid gap-3 sm:grid-cols-[1fr_auto_auto]"}>
      <label className="grid gap-1 text-sm">
        <span className="text-muted-foreground">Subject</span>
        <Select value={offeringId} onValueChange={(offering) => go({ offering })}>
          <SelectTrigger className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {offerings.map((offering) => (
              <SelectItem key={offering.id} value={offering.id}>
                {offering.code} · {offering.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </label>
      <label className="grid gap-1 text-sm">
        <span className="text-muted-foreground">Date</span>
        <Input
          type="date"
          value={date}
          max={today}
          onChange={(event) => event.target.value && go({ date: event.target.value })}
        />
      </label>
      <label className="grid gap-1 text-sm">
        <span className="text-muted-foreground">Period</span>
        <Select value={String(slot)} onValueChange={(value) => go({ slot: Number(value) })}>
          <SelectTrigger className="w-28">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PERIODS.map((period) => (
              <SelectItem key={period} value={String(period)}>
                Period {period}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </label>
      {choose && (
        <label className="grid gap-1 text-sm">
          <span className="text-muted-foreground">Session</span>
          <Select value={kind} onValueChange={(value) => go({ kind: value as SessionKind })}>
            <SelectTrigger className="w-32">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="theory">Theory</SelectItem>
              <SelectItem value="practical">Practical</SelectItem>
            </SelectContent>
          </Select>
        </label>
      )}
    </div>
  );
}
