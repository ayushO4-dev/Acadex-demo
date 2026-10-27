"use client";

import { Paperclip, Plus, X } from "lucide-react";
import { useRef, useState, useTransition } from "react";
import { toast } from "sonner";

import { NativeSelect } from "@/components/native-select";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import type { Severity, StudentRow } from "@/lib/api";

import { attachToNotice, publishNotice, searchStudents } from "./actions";

export type Program = { id: number; code: string; name: string };

type Audience = "everyone" | "program" | "batch" | "people";

/** datetime-local (the browser's zone) to ISO, or null. */
const iso = (value: string) => (value ? new Date(value).toISOString() : null);

/**
 * Exam cell and administration: post a notice to everyone, a program, a
 * batch and section of students, or named students; now or at a set time,
 * with an optional expiry and attachments.
 */
export function NoticeForm({ programs, onDone }: { programs: Program[]; onDone?: () => void }) {
  const [audience, setAudience] = useState<Audience>("everyone");
  const [people, setPeople] = useState<StudentRow[]>([]);
  const [query, setQuery] = useState("");
  const [matches, setMatches] = useState<StudentRow[]>([]);
  const [files, setFiles] = useState<File[]>([]);
  const [pending, startTransition] = useTransition();
  const [searching, startSearch] = useTransition();
  const form = useRef<HTMLFormElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  function search(value: string) {
    setQuery(value);
    startSearch(async () => setMatches(await searchStudents(value)));
  }

  function submit(data: FormData) {
    const get = (k: string) => String(data.get(k) ?? "").trim();
    startTransition(async () => {
      const published = await publishNotice({
        title: get("title"),
        body: get("body"),
        severity: get("severity") as Severity,
        program_id: audience === "program" || audience === "batch" ? Number(get("program_id")) || null : null,
        batch_year: audience === "batch" ? Number(get("batch_year")) || null : null,
        section: audience === "batch" ? get("section") || null : null,
        recipient_ids: audience === "people" ? people.map((p) => p.id) : [],
        publish_at: get("when") === "later" ? iso(get("publish_at")) : null,
        expires_at: iso(get("expires_at")),
      });
      if (!published.ok) {
        toast.error(published.message);
        return;
      }
      for (const file of files) {
        const body = new FormData();
        body.set("file", file);
        const attached = await attachToNotice(published.id, body);
        if (attached && !attached.ok) toast.error(attached.message);
      }
      toast.success(published.scheduled ? "Notice scheduled" : "Notice published; emails are on their way");
      form.current?.reset();
      setAudience("everyone");
      setPeople([]);
      setFiles([]);
      onDone?.();
    });
  }

  return (
    <form ref={form} action={submit} className="grid gap-4">
      <div className="grid gap-2">
        <Label htmlFor="title">Title</Label>
        <Input id="title" name="title" required maxLength={200} />
      </div>
      <div className="grid gap-2">
        <Label htmlFor="body">Message</Label>
        <Textarea id="body" name="body" required maxLength={10000} rows={4} />
      </div>

      <div className="grid gap-4 sm:grid-cols-2">
        <div className="grid gap-2">
          <Label htmlFor="severity">Severity</Label>
          <NativeSelect id="severity" name="severity" defaultValue="general">
            <option value="general">General</option>
            <option value="important">Important</option>
            <option value="critical">Critical: everyone emailed, must acknowledge</option>
          </NativeSelect>
        </div>
        <div className="grid gap-2">
          <Label htmlFor="audience">Audience</Label>
          <NativeSelect id="audience" value={audience} onChange={(e) => setAudience(e.target.value as Audience)}>
            <option value="everyone">Everyone (also on the public board)</option>
            <option value="program">A program</option>
            <option value="batch">A batch and section of students</option>
            <option value="people">Named students</option>
          </NativeSelect>
        </div>
      </div>

      {(audience === "program" || audience === "batch") && (
        <div className="grid gap-4 sm:grid-cols-3">
          <div className="grid gap-2 sm:col-span-1">
            <Label htmlFor="program_id">Program</Label>
            <NativeSelect id="program_id" name="program_id" required>
              {programs.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </NativeSelect>
          </div>
          {audience === "batch" && (
            <>
              <div className="grid gap-2">
                <Label htmlFor="batch_year">Batch (admission year)</Label>
                <Input id="batch_year" name="batch_year" type="number" min={2000} max={2100} required placeholder="2024" />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="section">Section (optional)</Label>
                <Input id="section" name="section" maxLength={1} placeholder="A" className="uppercase" />
              </div>
            </>
          )}
        </div>
      )}

      {audience === "people" && (
        <div className="grid gap-2">
          <Label htmlFor="people">Students</Label>
          {people.length > 0 && (
            <div className="flex flex-wrap gap-1">
              {people.map((p) => (
                <Badge key={p.id} variant="secondary" className="gap-1">
                  {p.display_name} <span className="font-mono">{p.roll_no}</span>
                  <button type="button" aria-label={`Remove ${p.display_name}`} onClick={() => setPeople((all) => all.filter((x) => x.id !== p.id))}>
                    <X className="size-3" />
                  </button>
                </Badge>
              ))}
            </div>
          )}
          <Input id="people" value={query} onChange={(e) => search(e.target.value)} placeholder="Search by name or roll number" autoComplete="off" />
          {query.trim().length >= 2 && (
            <ul className="divide-y rounded-md border text-sm">
              {searching && matches.length === 0 && <li className="text-muted-foreground px-3 py-2">Searching…</li>}
              {!searching && matches.length === 0 && <li className="text-muted-foreground px-3 py-2">No students found.</li>}
              {matches
                .filter((m) => !people.some((p) => p.id === m.id))
                .map((m) => (
                  <li key={m.id}>
                    <button
                      type="button"
                      className="hover:bg-muted w-full px-3 py-2 text-left"
                      onClick={() => {
                        setPeople((all) => [...all, m]);
                        setQuery("");
                        setMatches([]);
                      }}
                    >
                      {m.display_name} <span className="text-muted-foreground font-mono text-xs">{m.roll_no}</span>
                    </button>
                  </li>
                ))}
            </ul>
          )}
        </div>
      )}

      <div className="grid gap-4 sm:grid-cols-3">
        <div className="grid gap-2">
          <Label htmlFor="when">Publish</Label>
          <NativeSelect id="when" name="when" defaultValue="now">
            <option value="now">Now</option>
            <option value="later">At a set time</option>
          </NativeSelect>
        </div>
        <div className="grid gap-2">
          <Label htmlFor="publish_at">At (if scheduled)</Label>
          <Input id="publish_at" name="publish_at" type="datetime-local" />
        </div>
        <div className="grid gap-2">
          <Label htmlFor="expires_at">Expires (optional)</Label>
          <Input id="expires_at" name="expires_at" type="datetime-local" />
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <input
          ref={fileInput}
          type="file"
          multiple
          accept="application/pdf,image/png,image/jpeg"
          className="hidden"
          onChange={(e) => {
            const chosen = [...(e.target.files ?? [])];
            e.target.value = "";
            setFiles((all) => [...all, ...chosen].slice(0, 5));
          }}
        />
        <Button type="button" variant="outline" size="sm" onClick={() => fileInput.current?.click()} disabled={files.length >= 5}>
          <Paperclip />
          Attach files
        </Button>
        {files.map((f, i) => (
          <Badge key={`${f.name}-${i}`} variant="secondary" className="gap-1">
            {f.name}
            <button type="button" aria-label={`Remove ${f.name}`} onClick={() => setFiles((all) => all.filter((_, j) => j !== i))}>
              <X className="size-3" />
            </button>
          </Badge>
        ))}
      </div>

      <div>
        <Button type="submit" disabled={pending || (audience === "people" && people.length === 0)}>
          {pending ? "Publishing…" : "Publish notice"}
        </Button>
      </div>
    </form>
  );
}

/** The page header's Add button: the notice form in a dialog, closed once it's published. */
export function AddNoticeDialog({ programs }: { programs: Program[] }) {
  const [open, setOpen] = useState(false);
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button>
          <Plus />
          Add notice
        </Button>
      </DialogTrigger>
      <DialogContent className="max-h-[90svh] gap-6 overflow-y-auto p-6 sm:max-w-3xl sm:p-8 [&>[data-slot=dialog-close]]:top-4 [&>[data-slot=dialog-close]]:right-4">
        <DialogHeader>
          <DialogTitle>Post a notice</DialogTitle>
          <DialogDescription>
            Emailed to its audience by their preference. Critical notices go to everyone and must be acknowledged.
          </DialogDescription>
        </DialogHeader>
        <NoticeForm programs={programs} onDone={() => setOpen(false)} />
      </DialogContent>
    </Dialog>
  );
}
