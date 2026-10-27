import { headers } from "next/headers";
import { redirect } from "next/navigation";
import { cache } from "react";

import {
  API_URL,
  type AttendanceMonth,
  type AttendanceReport,
  type CalendarEvent,
  type Correction,
  type Session,
  type Student,
  type StudentPage,
  type LogPage,
  type Payable,
  type PaymentStatus,
  type Receipt,
  type Me,
  type NoticeList,
  type Offering,
  type Publication,
  type ResultsReport,
  type SetSubject,
  type ExternalSheet,
  type InternalSheet,
  type Roster,
  apiRequest,
  clientForwarding,
} from "@/lib/api";
import { getSession } from "@/lib/session";
import { clientHeadersOf } from "@/lib/request-context";

// Data for server components: calls the API as the signed-in user.

export async function asUser<T>(
  path: string,
  options: {
    method?: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
    body?: unknown;
    idempotencyKey?: string;
    recaptchaToken?: string;
  } = {},
): Promise<T> {
  return apiRequest<T>(path, { ...options, token: await accessToken(), client: clientHeadersOf(await headers()) });
}

/**
 * The signed-in user's API token. With no session (signed out, or expired),
 * send them to sign in rather than throw: pages render alongside their
 * layout, so a page can get here before the layout's own redirect.
 */
async function accessToken(): Promise<string> {
  const session = await getSession();
  if (!session) redirect("/");
  return session.token;
}

/** Once per request, however many components ask (layout, page, …). */
export const getMe = cache((): Promise<Me> => asUser<Me>("/me"));

/** One semester's attendance (default: the student's current semester). */
export function getAttendance(studentId: string, semester?: number): Promise<AttendanceReport> {
  const query = semester ? `?semester=${semester}` : "";
  return asUser<AttendanceReport>(`/students/${encodeURIComponent(studentId)}/attendance${query}`);
}

export function getOfferings(): Promise<Offering[]> {
  return asUser<Offering[]>("/offerings");
}

export function getRoster(offeringId: string, date: string, slot: number): Promise<Roster> {
  const query = new URLSearchParams({ date, slot: String(slot) });
  return asUser<Roster>(`/offerings/${encodeURIComponent(offeringId)}/roster?${query}`);
}

/** `query`: before, from, to, category, outcome, role, who (see the Logs page). */
export function getLogs(query: URLSearchParams): Promise<LogPage> {
  const qs = query.toString();
  return asUser<LogPage>(qs ? `/audit/logs?${qs}` : "/audit/logs");
}

/** Newest first; `before` continues from the last notice of a page. */
export function getNotices(before?: { at: string; id: string }): Promise<NoticeList> {
  const query = before ? `?${new URLSearchParams({ before: before.at, before_id: before.id })}` : "";
  return asUser<NoticeList>(`/notices${query}`);
}

export function getResults(studentId: string, semester?: number): Promise<ResultsReport> {
  const query = semester ? `?semester=${semester}` : "";
  return asUser<ResultsReport>(`/students/${encodeURIComponent(studentId)}/results${query}`);
}

export function getPublications(): Promise<Publication[]> {
  return asUser<Publication[]>("/results/publications");
}

export function getSetSubjects(setId: string): Promise<SetSubject[]> {
  return asUser<SetSubject[]>(`/results/publications/${encodeURIComponent(setId)}/subjects`);
}

export function getExternalSheet(setId: string, offeringId: string): Promise<ExternalSheet> {
  return asUser<ExternalSheet>(
    `/results/publications/${encodeURIComponent(setId)}/subjects/${encodeURIComponent(offeringId)}/marks`,
  );
}

export function getInternalSheet(offeringId: string): Promise<InternalSheet> {
  return asUser<InternalSheet>(`/offerings/${encodeURIComponent(offeringId)}/internal`);
}


export function getPayable(): Promise<Payable> {
  return asUser<Payable>("/fees/payable");
}

export function getPaymentStatus(attemptId: string): Promise<PaymentStatus> {
  return asUser<PaymentStatus>(`/fees/payments/${encodeURIComponent(attemptId)}`);
}

export function getReceipts(): Promise<Receipt[]> {
  return asUser<Receipt[]>("/fees/receipts");
}

/** The API's raw response, for binary content such as a receipt PDF. */
export async function fetchAsUser(path: string): Promise<Response> {
  return fetch(`${API_URL}${path}`, {
    headers: { authorization: `Bearer ${await accessToken()}`, ...forwarded(clientHeadersOf(await headers())) },
    cache: "no-store",
    signal: AbortSignal.timeout(15000),
  });
}

/** A long-lived response (server-sent events): no timeout; ends when the browser goes. */
export async function streamAsUser(path: string, signal: AbortSignal): Promise<Response> {
  return fetch(`${API_URL}${path}`, {
    headers: { authorization: `Bearer ${await accessToken()}`, accept: "text/event-stream" },
    cache: "no-store",
    signal,
  });
}

const forwarded = clientForwarding;

const query = (params: Record<string, string | number | undefined | null>) =>
  new URLSearchParams(Object.entries(params).filter(([, v]) => v !== undefined && v !== null && v !== "").map(([k, v]) => [k, String(v)]));

export function getCalendar(from: string, to: string): Promise<CalendarEvent[]> {
  return asUser<CalendarEvent[]>(`/academic/calendar?${query({ from, to })}`);
}

export function getAttendanceMonth(studentId: string, month?: string): Promise<AttendanceMonth> {
  return asUser<AttendanceMonth>(`/students/${encodeURIComponent(studentId)}/attendance/days?${query({ month })}`);
}

export function getSessions(studentId: string, subject: string, academicYear?: number): Promise<Session[]> {
  return asUser<Session[]>(
    `/students/${encodeURIComponent(studentId)}/attendance/sessions?${query({ subject, academic_year: academicYear })}`,
  );
}

export function getCorrections(): Promise<Correction[]> {
  return asUser<Correction[]>("/attendance/corrections");
}

export function findStudents(search?: string, after?: string): Promise<StudentPage> {
  return asUser<StudentPage>(`/students?${query({ search, after })}`);
}

export function getStudent(id: string): Promise<Student> {
  return asUser<Student>(`/students/${encodeURIComponent(id)}`);
}

/** Sends a multipart form to the API as the signed-in user. */
export async function postFormAsUser(path: string, form: FormData): Promise<Response> {
  return fetch(`${API_URL}${path}`, {
    method: "POST",
    headers: { authorization: `Bearer ${await accessToken()}`, ...forwarded(clientHeadersOf(await headers())) },
    body: form,
    cache: "no-store",
    signal: AbortSignal.timeout(30000),
  });
}
