"use server";

import { revalidatePath } from "next/cache";

import { asUser, findStudents, postFormAsUser } from "@/lib/acadex";
import { ApiError, type NoticeList, type NoticeReceipts, type Problem, type Severity, type StudentRow } from "@/lib/api";

export type ActionResult = { ok: true } | { ok: false; message: string } | null;
export type Published = { ok: true; id: string; scheduled: boolean } | { ok: false; message: string };

function failure(err: unknown): { ok: false; message: string } {
  const problem = err instanceof ApiError ? err.problem : null;
  const detail = problem?.detail && problem.detail !== problem.title ? `: ${problem.detail}` : "";
  return { ok: false, message: problem ? `${problem.title}${detail}` : "The Acadex API is unreachable" };
}

export async function markRead(ids: string[]): Promise<ActionResult> {
  try {
    await asUser("/notices/read", { method: "POST", body: { ids } });
  } catch (err) {
    return failure(err);
  }
  // The bell's count lives in the layout, so refresh that too.
  revalidatePath("/", "layout");
  return { ok: true };
}

const SEVERITIES: Severity[] = ["general", "important", "critical"];

export type NewNotice = {
  title: string;
  body: string;
  severity: Severity;
  program_id: number | null;
  batch_year: number | null;
  section: string | null;
  recipient_ids: string[];
  publish_at: string | null;
  expires_at: string | null;
};

export async function publishNotice(notice: NewNotice): Promise<Published> {
  if (!SEVERITIES.includes(notice.severity)) return { ok: false, message: "Choose a severity" };
  try {
    const created = await asUser<{ id: string; scheduled: boolean }>("/notices", { method: "POST", body: notice });
    revalidatePath("/", "layout");
    return { ok: true, ...created };
  } catch (err) {
    return failure(err);
  }
}

export async function attachToNotice(id: string, form: FormData): Promise<ActionResult> {
  const file = form.get("file");
  if (!(file instanceof File) || file.size === 0) return { ok: false, message: "Choose a file" };
  if (file.size > 5 * 1024 * 1024) return { ok: false, message: `${file.name} is over 5 MB` };
  const body = new FormData();
  body.set("file", file, file.name);
  const response = await postFormAsUser(`/notices/${encodeURIComponent(id)}/attachments`, body);
  if (!response.ok) {
    const problem = (await response.json().catch(() => null)) as Problem | null;
    return failure(new ApiError(response.status, problem));
  }
  revalidatePath("/notices");
  return { ok: true };
}

export async function searchStudents(query: string): Promise<StudentRow[]> {
  if (query.trim().length < 2) return [];
  try {
    return (await findStudents(query.trim())).students.slice(0, 8);
  } catch {
    return [];
  }
}

export async function receipts(id: string): Promise<NoticeReceipts | null> {
  try {
    return await asUser<NoticeReceipts>(`/notices/${encodeURIComponent(id)}/receipts`);
  } catch {
    return null;
  }
}

export async function acknowledge(id: string): Promise<ActionResult> {
  try {
    await asUser(`/notices/${encodeURIComponent(id)}/acknowledge`, { method: "POST" });
  } catch (err) {
    return failure(err);
  }
  revalidatePath("/", "layout");
  return { ok: true };
}

export async function setEmailLevel(level: "all" | "important" | "critical"): Promise<ActionResult> {
  try {
    await asUser("/notices/preferences", { method: "PUT", body: { email_level: level } });
  } catch (err) {
    return failure(err);
  }
  revalidatePath("/notices");
  return { ok: true };
}

/** For the bell's popover. */
export async function recentNotices(): Promise<NoticeList | null> {
  try {
    const list = await asUser<NoticeList>("/notices");
    return { ...list, notices: list.notices.slice(0, 15) };
  } catch {
    return null;
  }
}

export type NoticeEdit = { title: string; body: string; severity: Severity; expires_at: string | null };

export async function editNotice(id: string, edit: NoticeEdit): Promise<ActionResult> {
  try {
    await asUser(`/notices/${encodeURIComponent(id)}`, { method: "PUT", body: edit });
  } catch (err) {
    return failure(err);
  }
  revalidatePath("/", "layout");
  return { ok: true };
}

export async function withdrawNotice(id: string, reason: string): Promise<ActionResult> {
  try {
    await asUser(`/notices/${encodeURIComponent(id)}/withdraw`, { method: "POST", body: { reason } });
  } catch (err) {
    return failure(err);
  }
  revalidatePath("/", "layout");
  return { ok: true };
}

export async function deleteScheduledNotice(id: string): Promise<ActionResult> {
  try {
    await asUser(`/notices/${encodeURIComponent(id)}`, { method: "DELETE" });
  } catch (err) {
    return failure(err);
  }
  revalidatePath("/", "layout");
  return { ok: true };
}
