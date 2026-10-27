-- F5c: the accounts office. Fee structures behind the charges already on
-- every ledger, the reasons for existing concessions, some new ones, and
-- two students who paid their exam fee twice: one refunded, one waiting.
--
-- For the demo: Kavita (accounts) opens Reconciliation, sees the open
-- "paid twice" issue, and refunds it in one step; the earlier case shows
-- what a settled one looks like.

BEGIN;
SELECT setseed(0.9501);

CREATE TEMP TABLE seed_accounts AS SELECT id FROM identity.users WHERE email = 'accounts@northgate.example';

-- ── Fee structures: semester 5 (applied in July), semester 6 (a draft) ──────
INSERT INTO fees.fee_structures (program_id, academic_year, semester, created_by, created_at, applied_at, applied_by)
SELECT p.id, 2026, 5, a.id, timestamptz '2026-06-20 11:00+05:30', timestamptz '2026-07-01 10:00+05:30', a.id
FROM org.programs p CROSS JOIN seed_accounts a;
INSERT INTO fees.fee_structures (program_id, academic_year, semester, created_by, created_at)
SELECT p.id, 2026, 6, a.id, timestamptz '2026-09-28 16:00+05:30'
FROM org.programs p CROSS JOIN seed_accounts a;

INSERT INTO fees.fee_structure_lines (structure_id, fee_head, particulars, amount_paise, due_on)
SELECT s.id, l.fee_head, l.particulars, l.amount_paise, l.due_on
FROM fees.fee_structures s
CROSS JOIN (VALUES ('tuition',    'Tuition fee — Semester 5',          6250000, date '2026-07-31'),
                   ('library',    'Library & laboratory fee — 2026-27', 450000, date '2026-07-31'),
                   ('activities', 'Student activities fee — 2026-27',   150000, date '2026-08-31'),
                   ('exam',       'Examination fee — Semester 5',       240000, date '2026-10-20'))
     l(fee_head, particulars, amount_paise, due_on)
WHERE s.semester = 5;
INSERT INTO fees.fee_structure_lines (structure_id, fee_head, particulars, amount_paise, due_on)
SELECT s.id, l.fee_head, l.particulars, l.amount_paise, l.due_on
FROM fees.fee_structures s
CROSS JOIN (VALUES ('tuition', 'Tuition fee — Semester 6',     6250000, date '2027-01-15'),
                   ('exam',    'Examination fee — Semester 6',  240000, date '2027-04-20'))
     l(fee_head, particulars, amount_paise, due_on)
WHERE s.semester = 6;

-- The July charges came from the semester-5 structures.
UPDATE fees.ledger c SET structure_line_id = l.id, recorded_by = (SELECT id FROM seed_accounts)
FROM people.students st, fees.fee_structures s, fees.fee_structure_lines l
WHERE c.kind = 'charge' AND c.academic_year = 2026 AND st.user_id = c.student_id
  AND s.program_id = st.program_id AND s.semester = 5 AND l.structure_id = s.id
  AND l.particulars = c.particulars;

-- ── Concessions ──────────────────────────────────────────────────────────────
-- The merit scholarships seeded with the ledger, with their reasons.
INSERT INTO fees.concessions (ledger_id, student_id, category, reason, granted_by, granted_at)
SELECT e.id, e.student_id, 'merit', 'Merit scholarship (25%): top decile in semester 4, sanction AC/2026/14',
       a.id, timestamptz '2026-07-05 12:00+05:30'
FROM fees.ledger e CROSS JOIN seed_accounts a
WHERE e.kind = 'concession';
UPDATE fees.ledger SET recorded_by = (SELECT id FROM seed_accounts) WHERE kind = 'concession';

-- Post-matric scholarships (full tuition) for six students who haven't paid
-- it, and a sibling concession (10%) for four more.
CREATE TEMP TABLE seed_concede AS
SELECT c.id AS charge_id, c.student_id, c.amount_paise,
       row_number() OVER (ORDER BY st.roll_no) AS n
FROM fees.ledger c JOIN people.students st ON st.user_id = c.student_id
WHERE c.kind = 'charge' AND c.particulars = 'Tuition fee — Semester 5'
  AND st.roll_no NOT IN ('ECE24001', 'ECE24002')
  AND fees.pending_of(c.id) = c.amount_paise
  AND random() < 0.2;
DELETE FROM seed_concede WHERE n > 10;

WITH granted AS (
    INSERT INTO fees.ledger (student_id, academic_year, kind, charge_id, amount_paise, remarks, recorded_by, created_at)
    SELECT k.student_id, 2026, 'concession', k.charge_id,
           CASE WHEN k.n <= 6 THEN k.amount_paise ELSE k.amount_paise / 10 END,
           CASE WHEN k.n <= 6 THEN 'Post-matric scholarship, Govt. of Maharashtra (full tuition)'
                ELSE 'Sibling concession (10%): sibling enrolled in B.Tech' END,
           a.id, timestamptz '2026-08-12 15:00+05:30' + k.n * interval '1 day'
    FROM seed_concede k CROSS JOIN seed_accounts a
    RETURNING id, student_id, charge_id, remarks, created_at
)
INSERT INTO fees.concessions (ledger_id, student_id, category, reason, granted_by, granted_at)
SELECT g.id, g.student_id,
       CASE WHEN g.remarks LIKE 'Post-matric%' THEN 'government_scholarship' ELSE 'sibling' END,
       CASE WHEN g.remarks LIKE 'Post-matric%' THEN 'Post-matric scholarship (SC/ST), sanction MAHADBT/2026/' || (4100 + k.n)
            ELSE 'Sibling ' || 'enrolled in B.Tech, verified against admission records' END,
       (SELECT id FROM seed_accounts), g.created_at
FROM granted g JOIN seed_concede k ON k.charge_id = g.charge_id;

-- ── Paid twice ───────────────────────────────────────────────────────────────
-- Two students who had paid everything paid the exam fee again from a second
-- browser tab. Times are relative to seeding, after every receipt so far, so
-- the receipt numbers stay in time order.
CREATE TEMP TABLE seed_twice AS
SELECT st.user_id AS student_id, c.id AS charge_id, n,
       gen_random_uuid() AS attempt_id, gen_random_uuid() AS payment_id,
       greatest(now(), (SELECT max(issued_at) FROM fees.receipts) + interval '3 hours')
         - CASE n WHEN 1 THEN interval '150 minutes' ELSE interval '90 minutes' END AS paid_at
FROM (SELECT st.*, row_number() OVER (ORDER BY st.roll_no DESC) AS n
      FROM people.students st
      WHERE st.roll_no NOT IN ('ECE24001', 'ECE24002')
        AND EXISTS (SELECT 1 FROM fees.payments p WHERE p.student_id = st.user_id)
        AND NOT EXISTS (SELECT 1 FROM fees.ledger c WHERE c.student_id = st.user_id AND c.kind = 'charge'
                        AND fees.pending_of(c.id) <> 0)) st
JOIN fees.ledger c ON c.student_id = st.user_id AND c.kind = 'charge' AND c.fee_head = 'exam' AND c.academic_year = 2026
WHERE st.n <= 2;

INSERT INTO fees.payment_attempts (id, student_id, idempotency_key, amount_paise, gateway_order_id, status, created_at, updated_at)
SELECT attempt_id, student_id, 'seed-twice-' || n, 240000, 'order_seed_twice' || n, 'captured', paid_at - interval '2 minutes', paid_at
FROM seed_twice;
INSERT INTO fees.payment_attempt_items (attempt_id, charge_id, amount_paise)
SELECT attempt_id, charge_id, 240000 FROM seed_twice;
INSERT INTO fees.payments (id, attempt_id, student_id, gateway_payment_id, amount_paise, method, captured_at)
SELECT payment_id, attempt_id, student_id, 'pay_seed_twice' || n, 240000, 'upi', paid_at FROM seed_twice;
INSERT INTO fees.ledger (student_id, academic_year, kind, charge_id, amount_paise, payment_id, created_at)
SELECT student_id, 2026, 'payment', charge_id, 240000, payment_id, paid_at FROM seed_twice;
WITH numbered AS (
    SELECT t.*, (SELECT last_no FROM fees.receipt_seq WHERE financial_year = 2026)
                + row_number() OVER (ORDER BY t.paid_at) AS seq
    FROM seed_twice t
)
INSERT INTO fees.receipts (receipt_no, financial_year, seq, payment_id, student_id, amount_paise, remarks, issued_at)
SELECT 'RCPT/2026-27/' || lpad(seq::text, 6, '0'), 2026, seq, payment_id, student_id, 240000,
       'Examination fee — Semester 5', paid_at
FROM numbered;
UPDATE fees.receipt_seq SET last_no = (SELECT max(seq) FROM fees.receipts WHERE financial_year = 2026)
WHERE financial_year = 2026;
INSERT INTO ops.outbox (topic, ordering_key, payload)
SELECT 'receipt.requested', r.student_id::text, jsonb_build_object('receipt_id', r.id)
FROM fees.receipts r JOIN seed_twice t ON t.payment_id = r.payment_id;

-- Reconciliation runs every five minutes; the latest found both. Its window
-- ends now, so the first live run starts here.
INSERT INTO fees.reconciliation_runs (started_at, finished_at, window_from, window_to, gateway_payments, matched, issues_opened)
SELECT t, t + interval '2 seconds', t - interval '17 minutes', t - interval '2 minutes',
       CASE WHEN t > now() - interval '100 minutes' THEN 1 ELSE 0 END,
       CASE WHEN t > now() - interval '100 minutes' THEN 1 ELSE 0 END, 0
FROM generate_series(now() - interval '160 minutes', now() - interval '5 minutes', interval '5 minutes') t;
UPDATE fees.reconciliation_runs SET window_to = now(), finished_at = now()
WHERE id = (SELECT max(id) FROM fees.reconciliation_runs);

INSERT INTO fees.reconciliation_issues (kind, reference, student_id, payment_id, gateway_payment_id, charge_id,
                                        amount_paise, details, run_id, found_at)
SELECT 'overpaid', 'charge:' || t.charge_id, t.student_id, t.payment_id, 'pay_seed_twice' || t.n, t.charge_id, 240000,
       jsonb_build_object('via', 'webhook', 'receipt_no', r.receipt_no),
       (SELECT max(id) FROM fees.reconciliation_runs WHERE started_at <= t.paid_at + interval '5 minutes'),
       t.paid_at
FROM seed_twice t JOIN fees.receipts r ON r.payment_id = t.payment_id;
UPDATE fees.reconciliation_runs SET issues_opened = 1
WHERE id IN (SELECT run_id FROM fees.reconciliation_issues WHERE run_id IS NOT NULL);

-- The first was refunded ten minutes later.
INSERT INTO fees.refunds (student_id, payment_id, gateway_payment_id, amount_paise, reason_category, reason,
                          idempotency_key, status, gateway_refund_id, requested_by, requested_at, processed_at, issue_id)
SELECT t.student_id, t.payment_id, 'pay_seed_twice1', 240000, 'duplicate_payment',
       'Exam fee paid twice from two browser tabs', 'seed-refund-0001', 'processed', 'rfnd_seed_0001',
       a.id, t.paid_at + interval '10 minutes', t.paid_at + interval '10 minutes',
       (SELECT id FROM fees.reconciliation_issues WHERE reference = 'charge:' || t.charge_id)
FROM seed_twice t CROSS JOIN seed_accounts a WHERE t.n = 1;
INSERT INTO fees.ledger (student_id, academic_year, kind, charge_id, amount_paise, payment_id, remarks, recorded_by, created_at)
SELECT t.student_id, 2026, 'refund', t.charge_id, 240000, t.payment_id, 'Refund rfnd_seed_0001', a.id,
       t.paid_at + interval '10 minutes'
FROM seed_twice t CROSS JOIN seed_accounts a WHERE t.n = 1;
UPDATE fees.reconciliation_issues i
SET status = 'resolved', resolution = 'Refunded ₹2,400 (rfnd_seed_0001)', resolved_by = a.id,
    resolved_at = t.paid_at + interval '10 minutes'
FROM seed_twice t, seed_accounts a
WHERE t.n = 1 AND i.reference = 'charge:' || t.charge_id;

COMMIT;
