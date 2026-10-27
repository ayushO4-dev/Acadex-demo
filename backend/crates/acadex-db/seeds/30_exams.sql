-- F3a: the Nov–Dec 2026 semester-end session, its timetable and rooms, and
-- about 70% of students already paid up and registered, so seating and hall
-- tickets have real volume. Atharva (fees paid) submits his form live in the
-- demo; Sanika is blocked by her unpaid tuition.

BEGIN;

UPDATE fees.ledger SET fee_head = CASE
        WHEN particulars LIKE 'Tuition fee%' THEN 'tuition'
        WHEN particulars LIKE 'Library%' THEN 'library'
        WHEN particulars LIKE 'Student activities%' THEN 'activities'
        WHEN particulars LIKE 'Examination fee%' THEN 'exam'
        ELSE 'other' END
WHERE kind = 'charge' AND fee_head IS NULL;

INSERT INTO exams.exam_sessions (institution_id, name, academic_year, form_opens_on, form_closes_on, regular_fee_paise, backlog_fee_paise, created_by)
SELECT 1, 'Nov–Dec 2026 semester-end examinations', 2026, date '2026-10-01', date '2026-10-20', 240000, 100000, id
FROM identity.users WHERE email = 'examcell@northgate.example';

-- The form window, everywhere it's mentioned.
UPDATE academic.calendar_events SET starts_on = '2026-10-01' WHERE title = 'Semester 5 examination forms';
UPDATE comms.notices SET body = E'The examination form for Semester 5 is open from 1 October to 20 October.\nForms submitted late attract a late fee.'
WHERE title = 'Semester 5 examination form window';

INSERT INTO exams.venues (id, institution_id, name, capacity)
SELECT row_number() OVER (ORDER BY block, room), 1, block || room, 30
FROM (VALUES ('A'), ('B')) b(block), generate_series(101, 110) room;

-- Semester 5 on Mon/Wed/Fri mornings from 16 November (labs that afternoon);
-- backlog papers on the days between, semester 3 in the afternoon and
-- semester 1 in the morning, so no one sits two at once. No Sundays.
INSERT INTO exams.schedule (session_id, subject_id, exam_date, starts_at, duration_minutes)
SELECT x.id, s.id,
       date '2026-11-16' + (2 * k + CASE WHEN k >= 3 THEN 1 ELSE 0 END + CASE WHEN s.semester = 5 THEN 0 ELSE 1 END)::int,
       CASE WHEN s.semester = 3 OR s.kind = 'lab' THEN time '14:00' ELSE time '10:00' END,
       180
FROM (SELECT s.*, (row_number() OVER (PARTITION BY s.program_id, s.semester ORDER BY s.code) - 1) AS k
      FROM academic.subjects s WHERE s.semester IN (1, 3, 5)) s
CROSS JOIN exams.exam_sessions x;

-- Who has paid everything and registered: about 70%, never the demo
-- students or anyone with a payment already.
CREATE TEMP TABLE seed_paid_up AS
SELECT st.user_id AS student_id, gen_random_uuid() AS attempt_id, gen_random_uuid() AS payment_id,
       row_number() OVER (ORDER BY st.roll_no)::int AS n,
       timestamptz '2026-07-25 10:00+05:30' + (random() * interval '60 days') AS paid_at
FROM people.students st
WHERE st.roll_no NOT IN ('ECE24001', 'ECE24002')
  AND NOT EXISTS (SELECT 1 FROM fees.payments p WHERE p.student_id = st.user_id)
  AND random() < 0.7;

CREATE TEMP TABLE seed_owed AS
SELECT c.student_id, c.id AS charge_id, fees.pending_of(c.id) AS amount, c.particulars
FROM fees.ledger c JOIN seed_paid_up p ON p.student_id = c.student_id
WHERE c.kind = 'charge' AND c.academic_year = 2026 AND fees.pending_of(c.id) > 0;

INSERT INTO fees.payment_attempts (id, student_id, idempotency_key, amount_paise, gateway_order_id, status, created_at, updated_at)
SELECT p.attempt_id, p.student_id, 'seed-paid-up-' || p.n, sum(o.amount), 'order_seed_pu' || p.n, 'captured', p.paid_at, p.paid_at
FROM seed_paid_up p JOIN seed_owed o USING (student_id)
GROUP BY p.attempt_id, p.student_id, p.n, p.paid_at;
INSERT INTO fees.payment_attempt_items (attempt_id, charge_id, amount_paise)
SELECT p.attempt_id, o.charge_id, o.amount FROM seed_paid_up p JOIN seed_owed o USING (student_id);
INSERT INTO fees.payments (id, attempt_id, student_id, gateway_payment_id, amount_paise, method, captured_at)
SELECT p.payment_id, p.attempt_id, p.student_id, 'pay_seed_pu' || p.n, a.amount_paise,
       (ARRAY['upi', 'card', 'netbanking'])[1 + p.n % 3], p.paid_at
FROM seed_paid_up p JOIN fees.payment_attempts a ON a.id = p.attempt_id;
INSERT INTO fees.ledger (student_id, academic_year, kind, charge_id, amount_paise, payment_id, created_at)
SELECT p.student_id, 2026, 'payment', o.charge_id, o.amount, p.payment_id, p.paid_at
FROM seed_paid_up p JOIN seed_owed o USING (student_id);

-- Receipts continue the year's gapless sequence, in payment order.
WITH numbered AS (
    SELECT p.*, (SELECT last_no FROM fees.receipt_seq WHERE financial_year = 2026)
                + row_number() OVER (ORDER BY p.paid_at, p.n) AS seq
    FROM seed_paid_up p
)
INSERT INTO fees.receipts (receipt_no, financial_year, seq, payment_id, student_id, amount_paise, remarks, issued_at)
SELECT 'RCPT/2026-27/' || lpad(n.seq::text, 6, '0'), 2026, n.seq, n.payment_id, n.student_id, a.amount_paise,
       (SELECT string_agg(o.particulars, ', ' ORDER BY o.charge_id) FROM seed_owed o WHERE o.student_id = n.student_id),
       n.paid_at
FROM numbered n JOIN fees.payment_attempts a ON a.id = n.attempt_id;
UPDATE fees.receipt_seq SET last_no = (SELECT max(seq) FROM fees.receipts WHERE financial_year = 2026) WHERE financial_year = 2026;

-- Their regular forms, submitted, for this semester's subjects.
INSERT INTO exams.exam_forms (id, session_id, student_id, kind, fee_paise, status, submitted_at, created_at)
SELECT gen_random_uuid(), x.id, p.student_id, 'regular', 240000, 'submitted',
       timestamptz '2026-10-01 09:00+05:30' + random() * interval '2 days', timestamptz '2026-10-01 09:00+05:30'
FROM seed_paid_up p CROSS JOIN exams.exam_sessions x;
INSERT INTO exams.exam_form_subjects (form_id, subject_id, offering_id)
SELECT f.id, s.id, o.id
FROM exams.exam_forms f
JOIN people.students st ON st.user_id = f.student_id
JOIN academic.subjects s ON s.program_id = st.program_id AND s.semester = st.current_semester
JOIN academic.offerings o ON o.subject_id = s.id AND o.academic_year = 2026;
INSERT INTO exams.exam_form_charges (form_id, charge_id)
SELECT f.id, c.id FROM exams.exam_forms f
JOIN fees.ledger c ON c.student_id = f.student_id AND c.kind = 'charge' AND c.fee_head = 'exam' AND c.academic_year = 2026;

-- The worker makes their receipt PDFs.
INSERT INTO ops.outbox (topic, ordering_key, payload)
SELECT 'receipt.requested', r.student_id::text, jsonb_build_object('receipt_id', r.id)
FROM fees.receipts r JOIN seed_paid_up p ON p.payment_id = r.payment_id;

COMMIT;
