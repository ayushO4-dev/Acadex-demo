"use server";

import { revalidatePath } from "next/cache";

import { asUser } from "@/lib/acadex";
import { ApiError } from "@/lib/api";

export type Outcome = { ok: true } | { ok: false; message: string };

function failure(err: unknown): Outcome {
  const problem = err instanceof ApiError ? err.problem : null;
  return { ok: false, message: problem?.title ?? "The Acadex API is unreachable" };
}

export async function requestCorrection(request: {
  offering_id: string;
  session_date: string;
  slot: number;
  requested_status: string;
  reason: string;
}): Promise<Outcome> {
  try {
    await asUser("/attendance/corrections", { method: "POST", body: request });
  } catch (err) {
    return failure(err);
  }
  revalidatePath("/academic/attendance", "layout");
  return { ok: true };
}

export async function decideCorrection(id: string, approve: boolean, note: string): Promise<Outcome> {
  try {
    await asUser(`/attendance/corrections/${encodeURIComponent(id)}/decision`, { method: "POST", body: { approve, note } });
  } catch (err) {
    return failure(err);
  }
  revalidatePath("/academic/attendance", "layout");
  return { ok: true };
}
