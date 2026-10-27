// Server-side client for the Rust API. The browser never calls it directly
// (architecture.md §3.1), so the URL is not a NEXT_PUBLIC_ variable.
export const API_URL = process.env.ACADEX_API_URL ?? "http://localhost:8080";

/** The browser's address and user agent, forwarded so the API can audit and rate-limit them. */
export type ClientHeaders = {
  ip?: string | null;
  userAgent?: string | null;
};

/**
 * Headers telling the API who the browser is. The address goes in our own
 * header with the key the web app and API share (BFF_FORWARDING_KEY): the API
 * believes a forwarded address only with it, since anyone can reach the API
 * and write X-Forwarded-For.
 */
export function clientForwarding(client: ClientHeaders | undefined): Record<string, string> {
  const out: Record<string, string> = {};
  const key = process.env.BFF_FORWARDING_KEY;
  if (client?.ip && key) {
    out["x-acadex-client-ip"] = client.ip;
    out["x-acadex-forwarding-key"] = key;
  }
  if (client?.userAgent) out["user-agent"] = client.userAgent;
  return out;
}

/** RFC 9457 problem details, as the API returns them. */
export type Problem = {
  type: string;
  title: string;
  status: number;
  detail?: string | null;
  trace_id?: string | null;
};

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly problem: Problem | null,
  ) {
    super(problem?.title ?? `API responded ${status}`);
  }

  /** The last segment of the problem type, e.g. "no-account". */
  get kind(): string | undefined {
    return this.problem?.type.split("/").pop();
  }
}

export type Student = {
  id: string;
  display_name: string;
  roll_no: string;
  program_id: number;
  program_name: string;
  batch_year: number;
  current_semester: number;
};

export type Role =
  | "student"
  | "faculty"
  | "hod"
  | "exam_cell"
  | "accounts"
  | "support_agent"
  | "admin"
  | "super_admin"
  /** The vendor's operator: runs the deployment, not the university. */
  | "platform_admin";

export type Department = {
  id: number;
  code: string;
  name: string;
  hod_id: string | null;
  hod_name: string | null;
  programs: { id: number; name: string }[];
  students: number;
  faculty: number;
};

export type Me = {
  id: string;
  display_name: string;
  role: Role;
  permissions: string[];
  student: Student | null;
  /** The department an HOD heads. */
  department: Department | null;
  unread_notices: number;
  /** Their own. */
  email: string | null;
};

export type Totals = {
  conducted: number;
  attended: number;
  percentage: number;
  shortage: boolean;
};

export type AttendanceReport = {
  student: Student;
  academic_year: number;
  /** The semester shown; null for a whole academic year. */
  semester: number | null;
  /** Semesters so far, newest first. */
  semesters: number[];
  /** Where the semester's attendance ends (YYYY-MM-DD). */
  last_session_on: string | null;
  shortage_threshold_pct: number;
  overall: Totals;
  /** theory / practical: each component on its own, when it has sessions. `shortage` is set if either is short. */
  subjects: (Totals & { code: string; name: string; credits: number; theory: Totals | null; practical: Totals | null })[];
};

export type AttendanceStatus = "present" | "absent" | "late" | "excused";

export type Offering = {
  id: string;
  code: string;
  name: string;
  academic_year: number;
  program_name: string;
  semester: number;
  student_count: number;
  /** theory | lab | elective */
  kind: string;
  /** Has practicals marked separately (labs are all practical). */
  has_practical: boolean;
};

export type RosterEntry = {
  student_id: string;
  roll_no: string;
  display_name: string;
  status: AttendanceStatus | null;
};

export type Roster = {
  offering: Offering;
  session_date: string;
  slot: number;
  /** How the session was marked, if it has been. */
  session_kind: SessionKind | null;
  students: RosterEntry[];
};

export type SessionKind = "theory" | "practical";

export type LogEntry = {
  id: number;
  occurred_at: string;
  actor_name: string | null;
  actor_role: string | null;
  action: string;
  resource_type: string;
  resource_id: string | null;
  outcome: "success" | "denied" | "error";
  ip: string | null;
  user_agent: string | null;
  trace_id: string | null;
  metadata: Record<string, unknown>;
};

export type LogPage = { entries: LogEntry[]; next_before: number | null };

export type Severity = "general" | "important" | "critical";

export type PublicNotice = {
  id: string;
  severity: Severity;
  title: string;
  body: string;
  published_at: string;
};

export type NoticeAttachment = { id: string; name: string; content_type: string; size_bytes: number };

export type Notice = PublicNotice & {
  expires_at: string | null;
  /** null for institution-wide notices. */
  program_name: string | null;
  batch_year: number | null;
  section: string | null;
  /** Addressed to named people. */
  individual: boolean;
  read: boolean;
  /** Critical notices are acknowledged, not just read. */
  acknowledged: boolean;
  attachments: NoticeAttachment[];
  author: string | null;
  /** Corrected after publishing. */
  edited_at: string | null;
  /** Withdrawn: only publishers still see it. */
  withdrawn_at: string | null;
  withdraw_reason: string | null;
};

export type NoticeReceipts = {
  audience: number;
  read: number;
  acknowledged: number;
  emailed: number;
  readers: { display_name: string; role: string; roll_no: string | null; read_at: string | null; acknowledged_at: string | null }[];
};

export type NoticeList = {
  notices: Notice[];
  unread: number;
  /** [published_at, id] of the last one, when there are older notices. */
  next: [string, string] | null;
};

export type ComponentMark = { code: string; max: number; marks: number | null };

export type ResultKind = "regular" | "backlog" | "revaluation";

export type SubjectResult = {
  /** 1 for the regular attempt, then backlog attempts. */
  attempt: number;
  kind: ResultKind;
  published_at: string | null;
  code: string;
  name: string;
  credits: number;
  components: ComponentMark[];
  internal: number;
  internal_max: number;
  external: number;
  external_max: number;
  total: number;
  max: number;
  percentage: number;
  grade: string;
  points: number;
  passed: boolean;
};

export type SemesterSummary = {
  semester: number;
  academic_year: number;
  /** null while staged; only the exam cell sees those. */
  published_at: string | null;
  /** From each subject's latest result: a cleared backlog counts. */
  sgpa: number;
  /** As first published, from the regular attempts only. */
  sgpa_regular: number;
  cgpa: number;
  credits_attempted: number;
  credits_earned: number;
  passed: boolean;
};

export type ResultsReport = {
  student: Student;
  semesters: SemesterSummary[];
  /** subjects: each subject's latest result; history: every attempt. */
  selected: (SemesterSummary & { subjects: SubjectResult[]; history: SubjectResult[] }) | null;
};

export type Publication = {
  id: string;
  program_id: number;
  program_name: string;
  semester: number;
  academic_year: number;
  kind: ResultKind;
  staged_at: string;
  published_at: string | null;
  published_by: string | null;
  /** Who asked to publish; someone else must approve. */
  requested_by_id: string | null;
  requested_by: string | null;
  requested_at: string | null;
  students: number;
  subjects: number;
  /** A withdrawal asked for (published sets); someone else must approve. */
  withdrawal_requested_by_id: string | null;
  withdrawal_requested_by: string | null;
  withdrawal_requested_at: string | null;
  withdrawal_reason: string | null;
  /** Times it was withdrawn before. */
  withdrawals: number;
};

export type RevaluationKind = "revaluation" | "retotal";

export type RevaluationRequest = {
  id: string;
  student_id: string;
  student_name: string;
  roll_no: string;
  program_name: string;
  code: string;
  name: string;
  semester: number;
  academic_year: number;
  kind: RevaluationKind;
  reason: string;
  fee_paise: number;
  pending_paise: number;
  status: "payment_pending" | "submitted" | "decided";
  old_marks: number;
  external_max: number;
  /** For students: only once the outcome is published (or unchanged). */
  new_marks: number | null;
  remarks: string | null;
  /** The re-evaluation result set holding a changed mark. */
  publication_id: string | null;
  outcome_published_at: string | null;
  decided_by: string | null;
  decided_at: string | null;
  submitted_at: string | null;
  created_at: string;
};

export type SetSubject = { offering_id: string; code: string; name: string };

export type ExternalSheet = {
  publication: Publication;
  max_marks: number;
  students: { student_id: string; roll_no: string; display_name: string; attempt: number; marks: number | null }[];
};

export type MarksComponent = { id: string | null; code: string; max_marks: number; position?: number };

export type InternalSheet = {
  components: MarksComponent[];
  /** Published results lock the marks. */
  published: boolean;
  /** marks: component id → marks entered so far. */
  students: { student_id: string; roll_no: string; display_name: string; marks: Record<string, number> }[];
};

/** Money is integer paise throughout; format with formatPaise. */
export type ChargeLine = {
  charge_id: number;
  academic_year: number;
  particulars: string;
  due_on: string | null;
  remarks: string | null;
  receivable: number;
  concession: number;
  paid: number;
  /** Negative is an advance. */
  pending: number;
};

export type Payable = {
  academic_year: number;
  lines: ChargeLine[];
  totals: { receivable: number; concession: number; paid: number; pending: number };
};

export type Checkout = { attempt_id: string; amount_paise: number; checkout_url: string };

export type PaymentStatus = {
  id: string;
  amount_paise: number;
  status: "created" | "captured" | "failed" | "review";
  receipt_no: string | null;
};

export type Receipt = {
  id: string;
  receipt_no: string;
  issued_at: string;
  remarks: string;
  amount_paise: number;
  pdf_ready: boolean;
};

export async function apiRequest<T>(
  path: string,
  options: {
    token?: string;
    method?: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
    body?: unknown;
    client?: ClientHeaders;
    idempotencyKey?: string;
    /** From the browser's reCAPTCHA Enterprise, for bot-protected forms. */
    recaptchaToken?: string;
    /** Seconds Next.js may reuse the response; only for public, same-for-everyone data. */
    revalidate?: number;
  } = {},
): Promise<T> {
  const headers: Record<string, string> = {};
  if (options.token) headers.authorization = `Bearer ${options.token}`;
  if (options.body !== undefined) headers["content-type"] = "application/json";
  if (options.idempotencyKey) headers["idempotency-key"] = options.idempotencyKey;
  if (options.recaptchaToken) headers["x-recaptcha-token"] = options.recaptchaToken;
  Object.assign(headers, clientForwarding(options.client));

  const res = await fetch(`${API_URL}${path}`, {
    method: options.method ?? "GET",
    headers,
    body: options.body === undefined ? undefined : JSON.stringify(options.body),
    ...(options.revalidate ? { next: { revalidate: options.revalidate } } : { cache: "no-store" }),
    // Long enough to ride out an API instance starting up.
    signal: AbortSignal.timeout(12000),
  });
  const body: unknown = await res.json().catch(() => null);
  if (!res.ok) throw new ApiError(res.status, body as Problem | null);
  return body as T;
}

/** The university this deployment serves, for public pages. */
export type PublicInstitution = {
  name: string;
  short_name: string | null;
  /** Set when there's a logo; also its version, for the URL. */
  logo_updated_at: string | null;
  email_domains: string[];
};

export async function getInstitution(): Promise<PublicInstitution> {
  try {
    return await apiRequest("/institution", { revalidate: 60 });
  } catch {
    return { name: "Acadex", short_name: null, logo_updated_at: null, email_domains: [] };
  }
}

/**
 * Tells the API about a new sign-in: checks the person has an Acadex account,
 * and audits it.
 */
export async function recordSignIn(
  token: string,
  client: ClientHeaders | undefined,
): Promise<"ok" | "no-account" | "failed"> {
  try {
    await apiRequest("/sessions", { token, method: "POST", client });
    return "ok";
  } catch (err) {
    if (err instanceof ApiError && err.kind === "no-account") return "no-account";
    console.error("recording sign-in failed", err);
    return "failed";
  }
}

/** Called as the user signs out, so it shows on their Logs page. Best effort. */
export async function recordSignOut(token: string, client: ClientHeaders | undefined): Promise<void> {
  try {
    await apiRequest("/sessions", { token, method: "DELETE", client });
  } catch (err) {
    console.warn("recording sign-out failed", err);
  }
}

// ── Academic (F2) ──────────────────────────────────────────────────────────

export type SubjectKind = "theory" | "lab" | "elective";

export type Subject = {
  id: string;
  code: string;
  name: string;
  credits: number;
  kind: SubjectKind;
  /** Lectures with a practical component, attended and counted separately. */
  has_practical?: boolean;
  semester: number;
  academic_year: number | null;
  faculty: string | null;
};

/** The weekly class timetable. */
export type TimetableEntry = {
  id: string;
  offering_id: string;
  code: string;
  subject: string;
  program_id: number;
  program: string;
  semester: number;
  section: string | null;
  weekday: number;
  slot: number;
  session_kind: "theory" | "practical";
  room: string | null;
  faculty: string | null;
};

export type Week = {
  academic_year: number;
  view: "class" | "teacher";
  program_id: number | null;
  semester: number | null;
  section: string | null;
  periods: { slot: number; starts: string; ends: string }[];
  entries: TimetableEntry[];
  editable: boolean;
  programs: { id: number; code: string; name: string }[];
  offerings: { id: string; code: string; name: string; kind: SubjectKind; has_practical: boolean; faculty: string | null }[];
};

export type EventKind = "holiday" | "exam" | "deadline" | "event" | "class_suspension";

export type CalendarEvent = {
  id: string;
  kind: EventKind;
  title: string;
  description: string | null;
  starts_on: string;
  ends_on: string;
  program_id: number | null;
  program_name: string | null;
};

export type AttendanceDay = { date: string; sessions: number; present: number; late: number; absent: number; excused: number };
export type AttendanceMonth = { month: string; academic_year: number; days: AttendanceDay[] };

export type AttendanceStatusName = "present" | "absent" | "late" | "excused";

export type Session = {
  offering_id: string;
  date: string;
  slot: number;
  status: AttendanceStatusName;
  session_kind: SessionKind;
  marked_by: string | null;
  marked_at: string;
  correction: "pending" | "approved" | "rejected" | null;
  /** The period's clock times, "HH:MM", from the institution's settings. */
  starts_at: string | null;
  ends_at: string | null;
};

export type Correction = {
  id: string;
  student_id: string;
  student_name: string;
  roll_no: string;
  subject_code: string;
  subject_name: string;
  session_date: string;
  slot: number;
  current_status: AttendanceStatusName;
  requested_status: AttendanceStatusName;
  reason: string;
  status: "pending" | "approved" | "rejected";
  decided_by: string | null;
  decided_at: string | null;
  decision_note: string | null;
  created_at: string;
};

export type StudentRow = {
  id: string;
  display_name: string;
  roll_no: string;
  program_name: string;
  current_semester: number;
  section: string;
  graduated_on: string | null;
};

export type StudentPage = { students: StudentRow[]; next: string | null };

export type SupportStatus = "open" | "in_progress" | "waiting_on_requester" | "resolved" | "closed";
export type TicketPriority = "low" | "normal" | "high" | "urgent";

export type Ticket = {
  id: string;
  number: number;
  subject: string;
  category: string;
  priority: TicketPriority;
  status: SupportStatus;
  requester_id: string;
  requester_name: string;
  requester_role: string;
  requester_roll_no: string | null;
  assignee_id: string | null;
  assignee_name: string | null;
  linked_resource: string | null;
  created_at: string;
  updated_at: string;
  first_response_due: string;
  resolve_due: string;
  first_responded_at: string | null;
  resolved_at: string | null;
  escalation_level: number;
  last_message_at: string | null;
  messages: number;
  unread: boolean;
};

// ── Operations (F6) ────────────────────────────────────────────────────────

export type DeadLetter = {
  id: number;
  message_id: string;
  event: string;
  payload: Record<string, unknown>;
  attempts: number;
  last_error: string;
  first_failed_at: string;
  dead_at: string;
  status: "dead" | "replayed" | "discarded";
  handled_by: string | null;
  handled_at: string | null;
  note: string | null;
};

export type Operations = {
  audit_chain: {
    id: number;
    verified_at: string;
    rows_checked: number;
    head_id: number | null;
    first_broken_id: number | null;
    detail: string | null;
  } | null;
  outbox_pending: number;
  outbox_oldest_seconds: number | null;
  dead_letters: DeadLetter[];
};

export type Shortage = {
  student_id: string;
  student: string;
  roll_no: string;
  program: string;
  semester: number;
  subject_code: string;
  subject: string;
  session_kind: "theory" | "practical";
  conducted: number;
  attended: number;
  pct: number;
  refreshed_at: string;
};
