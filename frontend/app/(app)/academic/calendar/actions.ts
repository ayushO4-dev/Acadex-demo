"use server";

import { revalidatePath } from "next/cache";

import { asUser } from "@/lib/acadex";
import { ApiError } from "@/lib/api";

export type Outcome = { ok: true } | { ok: false; message: string } | null;

function failure(err: unknown): { ok: false; message: string } {
  const problem = err instanceof ApiError ? err.problem : null;
  const detail = problem?.detail && problem.detail !== problem.title ? ` (${problem.detail})` : "";
  return { ok: false, message: problem ? `${problem.title}${detail}` : "The Acadex API is unreachable" };
}

/** Adds an event, or with an id changes it. */
export async function saveEvent(id: string | null, _previous: Outcome, form: FormData): Promise<Outcome> {
  const program = String(form.get("program_id") ?? "");
  try {
    await asUser(id ? `/academic/calendar/${encodeURIComponent(id)}` : "/academic/calendar", {
      method: id ? "PUT" : "POST",
      body: {
        kind: String(form.get("kind")),
        title: String(form.get("title") ?? ""),
        description: String(form.get("description") ?? "").trim() || null,
        starts_on: String(form.get("starts_on")),
        ends_on: String(form.get("ends_on") || form.get("starts_on")),
        program_id: program ? Number(program) : null,
      },
    });
  } catch (err) {
    return failure(err);
  }
  revalidatePath("/academic/calendar");
  return { ok: true };
}

export async function removeEvent(id: string): Promise<Outcome> {
  try {
    await asUser(`/academic/calendar/${encodeURIComponent(id)}`, { method: "DELETE" });
  } catch (err) {
    return failure(err);
  }
  revalidatePath("/academic/calendar");
  return { ok: true };
}
