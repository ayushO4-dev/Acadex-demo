-- Access to fees. Same conventions as 0200_rls_policies.sql.
--
-- Besides the user roles, two system contexts act here, each set by the API
-- only on its own code path (rls::begin_system):
--   gateway  the payment webhook, after its signature is verified
--   worker   the receipt worker, filling in the PDF
-- They have no acting user, so policies that need one stay closed to them.

GRANT USAGE ON SCHEMA fees TO acadex_app;

-- ── fees.ledger ─────────────────────────────────────────────────────────────
-- SELECT and INSERT only: entries are never changed or removed.
GRANT SELECT, INSERT ON fees.ledger TO acadex_app;
ALTER TABLE fees.ledger ENABLE ROW LEVEL SECURITY;

CREATE POLICY student_reads_own ON fees.ledger FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id());
CREATE POLICY gateway_reads ON fees.ledger FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'gateway');
CREATE POLICY gateway_records_payments ON fees.ledger FOR INSERT TO acadex_app
    WITH CHECK (identity.current_app_role() = 'gateway' AND kind = 'payment');
CREATE POLICY worker_reads ON fees.ledger FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'worker');

-- ── fees.payment_attempts, fees.payment_attempt_items ───────────────────────
GRANT SELECT, INSERT ON fees.payment_attempts, fees.payment_attempt_items TO acadex_app;
GRANT UPDATE (gateway_order_id, status, updated_at) ON fees.payment_attempts TO acadex_app;
ALTER TABLE fees.payment_attempts ENABLE ROW LEVEL SECURITY;
ALTER TABLE fees.payment_attempt_items ENABLE ROW LEVEL SECURITY;

CREATE POLICY student_owns ON fees.payment_attempts FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id())
    WITH CHECK (identity.current_app_role() = 'student' AND student_id = identity.current_user_id()
                AND status = 'created');
CREATE POLICY gateway_settles ON fees.payment_attempts FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'gateway')
    WITH CHECK (identity.current_app_role() = 'gateway');

CREATE POLICY student_owns ON fees.payment_attempt_items FOR ALL TO acadex_app
    USING (attempt_id IN (SELECT id FROM fees.payment_attempts))  -- RLS-filtered: own attempts only
    WITH CHECK (identity.current_app_role() = 'student'
                AND attempt_id IN (SELECT id FROM fees.payment_attempts));
CREATE POLICY gateway_reads ON fees.payment_attempt_items FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'gateway');

-- ── fees.payments, fees.receipts ────────────────────────────────────────────
GRANT SELECT, INSERT ON fees.payments, fees.receipts TO acadex_app;
GRANT UPDATE (pdf_object) ON fees.receipts TO acadex_app;
ALTER TABLE fees.payments ENABLE ROW LEVEL SECURITY;
ALTER TABLE fees.receipts ENABLE ROW LEVEL SECURITY;

CREATE POLICY student_reads_own ON fees.payments FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id());
CREATE POLICY gateway_records ON fees.payments FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'gateway')
    WITH CHECK (identity.current_app_role() = 'gateway');
CREATE POLICY worker_reads ON fees.payments FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'worker');

CREATE POLICY student_reads_own ON fees.receipts FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id());
CREATE POLICY gateway_issues ON fees.receipts FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'gateway')
    WITH CHECK (identity.current_app_role() = 'gateway' AND pdf_object IS NULL);
CREATE POLICY worker_attaches_pdf ON fees.receipts FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'worker')
    WITH CHECK (identity.current_app_role() = 'worker');

-- ── fees.receipt_seq, fees.gateway_events: the webhook's alone ──────────────
GRANT SELECT, INSERT, UPDATE ON fees.receipt_seq TO acadex_app;
GRANT SELECT, INSERT ON fees.gateway_events TO acadex_app;
ALTER TABLE fees.receipt_seq ENABLE ROW LEVEL SECURITY;
ALTER TABLE fees.gateway_events ENABLE ROW LEVEL SECURITY;
CREATE POLICY gateway_only ON fees.receipt_seq FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'gateway')
    WITH CHECK (identity.current_app_role() = 'gateway');
CREATE POLICY gateway_only ON fees.gateway_events FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'gateway')
    WITH CHECK (identity.current_app_role() = 'gateway');

-- ── Elsewhere ───────────────────────────────────────────────────────────────
-- The receipt PDF prints the student's name, roll number and program.
CREATE POLICY worker_reads ON people.students FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'worker');

-- System actions about a student (a payment confirmed by the gateway) show
-- on that student's Logs page; the entry names them in metadata.student_id.
CREATE POLICY subject_reads_system_actions ON audit.action_log FOR SELECT TO acadex_app
    USING (actor_id IS NULL
           AND metadata->>'student_id' = identity.current_user_id()::text);
