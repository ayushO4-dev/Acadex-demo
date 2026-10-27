"use server";

import { revalidatePath } from "next/cache";

import { asUser } from "@/lib/acadex";
import { ApiError, type AttendanceStatus, type SessionKind } from "@/lib/api";

export type MarkResult = { ok: true; saved: number } | { ok: false; message: string };

/** Saves one session's attendance through the API, as the signed-in faculty member. */
export async function saveAttendance(
  offeringId: string,
  sessionDate: string,
  slot: number,
  sessionKind: SessionKind,
  marks: { student_id: string; status: AttendanceStatus }[],
): Promise<MarkResult> {
  try {
    const { saved } = await asUser<{ saved: number }>(
      `/offerings/${encodeURIComponent(offeringId)}/attendance`,
      { method: "PUT", body: { session_date: sessionDate, slot, session_kind: sessionKind, marks } },
    );
    revalidatePath("/academic/attendance");
    return { ok: true, saved };
  } catch (err) {
    const problem = err instanceof ApiError ? err.problem : null;
    if (!problem) return { ok: false, message: "The Acadex API is unreachable" };
    return { ok: false, message: problem.detail ? `${problem.title}: ${problem.detail}` : problem.title };
  }
}
