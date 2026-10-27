"use server";

import { revalidatePath } from "next/cache";

import { asUser } from "@/lib/acadex";
import { ApiError, type ResultKind } from "@/lib/api";

export type ActionResult = { ok: true } | { ok: false; message: string };
export type SaveResult = { ok: true; saved: number } | { ok: false; message: string };

function failure(err: unknown): { ok: false; message: string } {
  const problem = err instanceof ApiError ? err.problem : null;
  return { ok: false, message: problem?.detail ?? problem?.title ?? "The Acadex API is unreachable" };
}

const set = (id: string) => `/results/publications/${encodeURIComponent(id)}`;

export async function createResultSet(input: {
  program_id: number;
  semester: number;
  academic_year: number;
  kind: ResultKind;
}): Promise<ActionResult & { id?: string }> {
  try {
    const created = await asUser<{ id: string }>("/results/publications", { method: "POST", body: input });
    revalidatePath("/examination/results");
    return { ok: true, id: created.id };
  } catch (err) {
    return failure(err);
  }
}

/** Step one of two: someone else in the exam cell approves. */
export async function requestPublishing(publicationId: string): Promise<ActionResult> {
  try {
    await asUser(`${set(publicationId)}/request`, { method: "POST" });
  } catch (err) {
    return failure(err);
  }
  revalidatePath("/examination/results", "layout");
  return { ok: true };
}

export async function withdrawRequest(publicationId: string): Promise<ActionResult> {
  try {
    await asUser(`${set(publicationId)}/request`, { method: "DELETE" });
  } catch (err) {
    return failure(err);
  }
  revalidatePath("/examination/results", "layout");
  return { ok: true };
}

export async function publishResults(publicationId: string): Promise<ActionResult> {
  try {
    await asUser(`${set(publicationId)}/publish`, { method: "POST" });
  } catch (err) {
    return failure(err);
  }
  // Publishing also posts a notice, so the bell's count changes too.
  revalidatePath("/", "layout");
  return { ok: true };
}

export async function saveExternalMarks(
  publicationId: string,
  offeringId: string,
  entries: { student_id: string; attempt: number; marks: number }[],
): Promise<SaveResult> {
  try {
    const { saved } = await asUser<{ saved: number }>(
      `${set(publicationId)}/subjects/${encodeURIComponent(offeringId)}/marks`,
      { method: "PUT", body: entries },
    );
    revalidatePath(`/examination/results/${publicationId}`);
    return { ok: true, saved };
  } catch (err) {
    return failure(err);
  }
}

export async function saveComponents(
  offeringId: string,
  components: { id: string | null; code: string; max_marks: number }[],
): Promise<ActionResult> {
  try {
    await asUser(`/offerings/${encodeURIComponent(offeringId)}/components`, { method: "PUT", body: components });
  } catch (err) {
    return failure(err);
  }
  revalidatePath(`/examination/results/internal/${offeringId}`);
  return { ok: true };
}

export async function saveInternalMarks(
  offeringId: string,
  entries: { student_id: string; component_id: string; marks: number }[],
): Promise<SaveResult> {
  try {
    const { saved } = await asUser<{ saved: number }>(`/offerings/${encodeURIComponent(offeringId)}/internal`, {
      method: "PUT",
      body: entries,
    });
    revalidatePath(`/examination/results/internal/${offeringId}`);
    return { ok: true, saved };
  } catch (err) {
    return failure(err);
  }
}

/** Withdrawing published results, step one: ask, with what's wrong. */
export async function requestWithdrawal(publicationId: string, reason: string): Promise<ActionResult> {
  try {
    await asUser(`${set(publicationId)}/withdrawal`, { method: "POST", body: { reason } });
  } catch (err) {
    return failure(err);
  }
  revalidatePath("/examination/results", "layout");
  return { ok: true };
}

export async function cancelWithdrawal(publicationId: string): Promise<ActionResult> {
  try {
    await asUser(`${set(publicationId)}/withdrawal`, { method: "DELETE" });
  } catch (err) {
    return failure(err);
  }
  revalidatePath("/examination/results", "layout");
  return { ok: true };
}

/** Step two, by someone else: back to staged, grade cards down, students told. */
export async function approveWithdrawal(publicationId: string): Promise<ActionResult> {
  try {
    await asUser(`${set(publicationId)}/withdrawal/approve`, { method: "POST" });
  } catch (err) {
    return failure(err);
  }
  revalidatePath("/", "layout");
  return { ok: true };
}
