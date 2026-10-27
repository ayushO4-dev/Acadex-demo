-- F3c: re-evaluation history and grade cards.
--
-- After the semester 3 results (22 January 2026), Atharva asked for EC305 to
-- be re-evaluated: 37 became 42, published in a re-evaluation set on
-- 7 February. Sanika asked for a re-total of EC303: no change. Both paid
-- online, so they have receipts in FY 2025-26.
-- Every published result set gets a results.published event, so the worker
-- makes everyone's grade cards.

BEGIN;

CREATE TEMP TABLE seed_reval AS
SELECT st.user_id AS student_id, o.id AS offering_id, em.marks AS old_marks, em.max_marks, v.kind, v.reason,
       v.fee, v.new_marks, v.remarks, v.n, gen_random_uuid() AS request_id, gen_random_uuid() AS attempt_id,
       gen_random_uuid() AS payment_id, s.code
FROM (VALUES (1, 'ECE24001', 'EC305', 'revaluation', 'I answered all of section B but the marks look like one answer was missed.',
              50000, 42.0, 'Answer 6(b) had not been marked.'),
             (2, 'ECE24002', 'EC303', 'retotal', 'Please recount the marks on my answer sheet; I expected a little more.',
              20000, 31.0, 'Totals checked; no change.'))
     v(n, roll_no, code, kind, reason, fee, new_marks, remarks)
JOIN people.students st ON st.roll_no = v.roll_no
JOIN academic.subjects s ON s.program_id = st.program_id AND s.code = v.code
JOIN academic.offerings o ON o.subject_id = s.id
JOIN exams.external_marks em ON em.student_id = st.user_id AND em.offering_id = o.id AND em.attempt = 1;

-- The fee, paid at once through the gateway.
CREATE TEMP TABLE seed_reval_charge AS
WITH c AS (
    INSERT INTO fees.ledger (student_id, academic_year, kind, particulars, amount_paise, fee_head, due_on, created_at)
    SELECT student_id, 2025, 'charge',
           CASE kind WHEN 'revaluation' THEN 'Re-evaluation fee — ' ELSE 'Re-total fee — ' END || code,
           fee, 'revaluation', date '2026-01-26', timestamptz '2026-01-26 11:00+05:30'
    FROM seed_reval
    RETURNING id, student_id
)
SELECT c.id AS charge_id, r.* FROM c JOIN seed_reval r USING (student_id);

INSERT INTO fees.payment_attempts (id, student_id, idempotency_key, amount_paise, gateway_order_id, status, created_at, updated_at)
SELECT attempt_id, student_id, 'revaluation-' || request_id, fee, 'order_seed_rv' || n, 'captured',
       timestamptz '2026-01-26 11:00+05:30', timestamptz '2026-01-26 11:02+05:30'
FROM seed_reval_charge;
INSERT INTO fees.payment_attempt_items (attempt_id, charge_id, amount_paise)
SELECT attempt_id, charge_id, fee FROM seed_reval_charge;
INSERT INTO fees.payments (id, attempt_id, student_id, gateway_payment_id, amount_paise, method, captured_at)
SELECT payment_id, attempt_id, student_id, 'pay_seed_rv' || n, fee, 'upi', timestamptz '2026-01-26 11:02+05:30'
FROM seed_reval_charge;
INSERT INTO fees.ledger (student_id, academic_year, kind, charge_id, amount_paise, payment_id, created_at)
SELECT student_id, 2025, 'payment', charge_id, fee, payment_id, timestamptz '2026-01-26 11:02+05:30'
FROM seed_reval_charge;

INSERT INTO fees.receipt_seq (financial_year, last_no) VALUES (2025, 0) ON CONFLICT DO NOTHING;
INSERT INTO fees.receipts (receipt_no, financial_year, seq, payment_id, student_id, amount_paise, remarks, issued_at)
SELECT 'RCPT/2025-26/' || lpad((r.last_no + c.n)::text, 6, '0'), 2025, r.last_no + c.n, c.payment_id, c.student_id, c.fee,
       CASE c.kind WHEN 'revaluation' THEN 'Re-evaluation fee — ' ELSE 'Re-total fee — ' END || c.code,
       timestamptz '2026-01-26 11:02+05:30'
FROM seed_reval_charge c CROSS JOIN fees.receipt_seq r WHERE r.financial_year = 2025;
UPDATE fees.receipt_seq SET last_no = (SELECT max(seq) FROM fees.receipts WHERE financial_year = 2025) WHERE financial_year = 2025;

-- The changed mark, in its own re-evaluation set: the office asked, the
-- Controller approved.
INSERT INTO exams.result_publications (program_id, semester, academic_year, kind, staged_at,
                                       requested_by, requested_at, published_at, published_by)
SELECT 11, 3, 2025, 'revaluation', timestamptz '2026-02-05 15:00+05:30',
       office.id, timestamptz '2026-02-06 10:00+05:30', timestamptz '2026-02-07 10:00+05:30', coe.id
FROM identity.users office, identity.users coe
WHERE office.email = 'examcell@northgate.example' AND coe.email = 'coe@northgate.example';

INSERT INTO exams.external_marks (student_id, offering_id, publication_id, marks, max_marks, attempt)
SELECT r.student_id, r.offering_id, rp.id, r.new_marks, r.max_marks, 2
FROM seed_reval r
JOIN exams.result_publications rp ON rp.program_id = 11 AND rp.semester = 3 AND rp.kind = 'revaluation'
WHERE r.new_marks <> r.old_marks;

INSERT INTO exams.revaluation_requests (id, student_id, offering_id, attempt, kind, reason, fee_paise, charge_id,
                                        status, submitted_at, old_marks, new_marks, publication_id, remarks,
                                        decided_by, decided_at, created_at)
SELECT c.request_id, c.student_id, c.offering_id, 1, c.kind, c.reason, c.fee, c.charge_id,
       'decided', timestamptz '2026-01-26 11:02+05:30', c.old_marks, c.new_marks,
       CASE WHEN c.new_marks <> c.old_marks THEN rp.id END, c.remarks,
       office.id, timestamptz '2026-02-05 15:00+05:30', timestamptz '2026-01-26 11:00+05:30'
FROM seed_reval_charge c
CROSS JOIN identity.users office
LEFT JOIN exams.result_publications rp ON rp.program_id = 11 AND rp.semester = 3 AND rp.kind = 'revaluation'
WHERE office.email = 'examcell@northgate.example';

-- Receipt PDFs, and grade cards for every published set, oldest first.
INSERT INTO ops.outbox (topic, ordering_key, payload)
SELECT 'receipt.requested', r.student_id::text, jsonb_build_object('receipt_id', r.id)
FROM fees.receipts r JOIN seed_reval_charge c ON c.payment_id = r.payment_id;
INSERT INTO ops.outbox (topic, ordering_key, payload)
SELECT 'results.published', rp.id::text,
       jsonb_build_object('publication_id', rp.id, 'program_id', rp.program_id, 'semester', rp.semester,
                          'academic_year', rp.academic_year, 'kind', rp.kind)
FROM exams.result_publications rp
WHERE rp.published_at IS NOT NULL
ORDER BY rp.published_at;

COMMIT;
