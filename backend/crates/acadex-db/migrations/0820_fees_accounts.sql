-- The accounts office (architecture.md §9.4, §11.2): fee structures that
-- charge a whole class at once, concessions, refunds, and reconciliation of
-- the gateway's settlements against what Acadex recorded.

-- ── Who recorded an entry ────────────────────────────────────────────────────
-- Charges from a structure, concessions and refunds are recorded by a person
-- in the accounts office; the webhook's payments by nobody.
ALTER TABLE fees.ledger ADD COLUMN recorded_by uuid REFERENCES identity.users;
GRANT SELECT (recorded_by), INSERT (recorded_by) ON fees.ledger TO acadex_app;

-- ── Fee structures ───────────────────────────────────────────────────────────
-- What a program charges its students in one semester of an academic year.
-- Applying it charges every student then in that semester, once per line.
CREATE TABLE fees.fee_structures (
    id            serial PRIMARY KEY,
    program_id    smallint NOT NULL REFERENCES org.programs,
    academic_year smallint NOT NULL,
    semester      smallint NOT NULL CHECK (semester BETWEEN 1 AND 8),
    created_by    uuid NOT NULL REFERENCES identity.users,
    created_at    timestamptz NOT NULL DEFAULT now(),
    -- Last time it was applied; applying again charges only what's missing.
    applied_at    timestamptz,
    applied_by    uuid REFERENCES identity.users,
    UNIQUE (program_id, academic_year, semester)
);

CREATE TABLE fees.fee_structure_lines (
    id           serial PRIMARY KEY,
    structure_id integer NOT NULL REFERENCES fees.fee_structures ON DELETE CASCADE,
    fee_head     text NOT NULL CHECK (fee_head IN ('tuition', 'library', 'activities', 'exam', 'other')),
    particulars  text NOT NULL CHECK (length(particulars) BETWEEN 3 AND 120),
    amount_paise bigint NOT NULL CHECK (amount_paise > 0 AND amount_paise <= 100000000),
    due_on       date NOT NULL,
    UNIQUE (structure_id, particulars)
);

ALTER TABLE fees.ledger ADD COLUMN structure_line_id integer REFERENCES fees.fee_structure_lines;
GRANT SELECT (structure_line_id), INSERT (structure_line_id) ON fees.ledger TO acadex_app;
-- Applying a structure twice charges nobody twice.
CREATE UNIQUE INDEX ledger_one_charge_per_line ON fees.ledger (student_id, structure_line_id)
    WHERE structure_line_id IS NOT NULL;

-- ── Concessions ──────────────────────────────────────────────────────────────
-- The ledger entry is the money; this says why it was given and by whom.
CREATE TABLE fees.concessions (
    ledger_id  bigint PRIMARY KEY REFERENCES fees.ledger,
    student_id uuid NOT NULL REFERENCES people.students,
    category   text NOT NULL CHECK (category IN ('merit', 'government_scholarship', 'sibling', 'staff_ward',
                                                 'sports', 'financial_hardship', 'other')),
    reason     text NOT NULL CHECK (length(reason) BETWEEN 3 AND 500),
    granted_by uuid REFERENCES identity.users,
    granted_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ON fees.concessions (student_id);

-- ── Refunds ──────────────────────────────────────────────────────────────────
-- Requested by accounts, sent to the gateway, then recorded on the ledger
-- once the gateway confirms. A payment the gateway took twice for one order
-- was never recorded as ours, so a refund names the gateway's payment id and
-- only sometimes one of our payments.
CREATE TABLE fees.refunds (
    id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    student_id         uuid NOT NULL REFERENCES people.students,
    payment_id         uuid REFERENCES fees.payments,
    gateway_payment_id text NOT NULL,
    amount_paise       bigint NOT NULL CHECK (amount_paise > 0),
    reason_category    text NOT NULL CHECK (reason_category IN ('duplicate_payment', 'excess_payment',
                                                                  'withdrawal', 'other')),
    reason             text NOT NULL CHECK (length(reason) BETWEEN 3 AND 500),
    -- Per click, like a payment: a retried request finds the same refund.
    idempotency_key    text NOT NULL UNIQUE CHECK (length(idempotency_key) BETWEEN 8 AND 100),
    status             text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'processed', 'failed')),
    gateway_refund_id  text UNIQUE,
    failure            text,
    requested_by       uuid NOT NULL REFERENCES identity.users,
    requested_at       timestamptz NOT NULL DEFAULT now(),
    processed_at       timestamptz
);
CREATE INDEX ON fees.refunds (gateway_payment_id);
CREATE INDEX ON fees.refunds (student_id);

-- ── Reconciliation ───────────────────────────────────────────────────────────
CREATE TABLE fees.reconciliation_runs (
    id               serial PRIMARY KEY,
    started_at       timestamptz NOT NULL DEFAULT now(),
    finished_at      timestamptz,
    -- Gateway payments created in [window_from, window_to) were compared.
    window_from      timestamptz NOT NULL,
    window_to        timestamptz NOT NULL,
    gateway_payments integer NOT NULL DEFAULT 0,
    matched          integer NOT NULL DEFAULT 0,
    recovered        integer NOT NULL DEFAULT 0,
    issues_opened    integer NOT NULL DEFAULT 0,
    error            text
);

-- Something that doesn't add up, for a person in accounts to settle.
--   missed_webhook      the gateway took a payment we hadn't heard of; recovered automatically
--   duplicate_capture   the gateway took a second payment for an order already paid
--   overpaid            two payments settled one charge: more paid than owed
--   amount_mismatch     the gateway's amount differs from ours
--   missing_at_gateway  we recorded a payment the gateway doesn't list
--   unknown_payment     a gateway payment for no order of ours
CREATE TABLE fees.reconciliation_issues (
    id                 serial PRIMARY KEY,
    kind               text NOT NULL CHECK (kind IN ('missed_webhook', 'duplicate_capture', 'overpaid',
                                                     'amount_mismatch', 'missing_at_gateway', 'unknown_payment')),
    -- What it's about, so the same problem is reported once: a gateway
    -- payment id, or 'charge:<id>' for an overpaid charge.
    reference          text NOT NULL,
    student_id         uuid REFERENCES people.students,
    payment_id         uuid REFERENCES fees.payments,
    gateway_payment_id text,
    charge_id          bigint REFERENCES fees.ledger,
    amount_paise       bigint,
    details            jsonb NOT NULL DEFAULT '{}',
    run_id             integer REFERENCES fees.reconciliation_runs,
    found_at           timestamptz NOT NULL DEFAULT now(),
    status             text NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'resolved')),
    resolution         text,
    resolved_by        uuid REFERENCES identity.users,
    resolved_at        timestamptz,
    UNIQUE (kind, reference)
);
CREATE INDEX ON fees.reconciliation_issues (status, found_at DESC);

-- A refund made to settle an issue resolves it when the gateway confirms.
ALTER TABLE fees.refunds ADD COLUMN issue_id integer REFERENCES fees.reconciliation_issues;

-- ── Access ───────────────────────────────────────────────────────────────────
-- The accounts office charges, concedes and refunds; it never records a
-- payment (only the gateway does), and every entry carries its name.
CREATE POLICY accounts_records ON fees.ledger FOR INSERT TO acadex_app
    WITH CHECK (identity.current_app_role() = 'accounts'
                AND kind IN ('charge', 'concession', 'refund')
                AND recorded_by = identity.current_user_id());
-- Reconciliation recovers a missed webhook in the gateway's context; it reads
-- and records exactly what the webhook would.

GRANT SELECT, INSERT, UPDATE, DELETE ON fees.fee_structures, fees.fee_structure_lines TO acadex_app;
GRANT USAGE ON SEQUENCE fees.fee_structures_id_seq, fees.fee_structure_lines_id_seq TO acadex_app;
ALTER TABLE fees.fee_structures ENABLE ROW LEVEL SECURITY;
ALTER TABLE fees.fee_structure_lines ENABLE ROW LEVEL SECURITY;
CREATE POLICY accounts_manages ON fees.fee_structures FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'accounts')
    WITH CHECK (identity.current_app_role() = 'accounts');
CREATE POLICY accounts_manages ON fees.fee_structure_lines FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'accounts')
    WITH CHECK (identity.current_app_role() = 'accounts');

GRANT SELECT, INSERT ON fees.concessions TO acadex_app;
ALTER TABLE fees.concessions ENABLE ROW LEVEL SECURITY;
CREATE POLICY accounts_grants ON fees.concessions FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'accounts')
    WITH CHECK (identity.current_app_role() = 'accounts' AND granted_by = identity.current_user_id());
CREATE POLICY student_reads_own ON fees.concessions FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id());

GRANT SELECT, INSERT ON fees.refunds TO acadex_app;
GRANT UPDATE (status, gateway_refund_id, failure, processed_at) ON fees.refunds TO acadex_app;
ALTER TABLE fees.refunds ENABLE ROW LEVEL SECURITY;
CREATE POLICY accounts_refunds ON fees.refunds FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'accounts')
    WITH CHECK (identity.current_app_role() = 'accounts' AND requested_by = identity.current_user_id());
CREATE POLICY student_reads_own ON fees.refunds FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id());
CREATE POLICY worker_reads ON fees.refunds FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'worker');

GRANT SELECT, INSERT, UPDATE ON fees.reconciliation_runs, fees.reconciliation_issues TO acadex_app;
GRANT USAGE ON SEQUENCE fees.reconciliation_runs_id_seq, fees.reconciliation_issues_id_seq TO acadex_app;
ALTER TABLE fees.reconciliation_runs ENABLE ROW LEVEL SECURITY;
ALTER TABLE fees.reconciliation_issues ENABLE ROW LEVEL SECURITY;
CREATE POLICY accounts_reads ON fees.reconciliation_runs FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'accounts');
CREATE POLICY worker_runs ON fees.reconciliation_runs FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'worker')
    WITH CHECK (identity.current_app_role() = 'worker');
CREATE POLICY accounts_resolves ON fees.reconciliation_issues FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'accounts')
    WITH CHECK (identity.current_app_role() = 'accounts');
-- The worker finds issues; the webhook reports a duplicate capture as it happens.
CREATE POLICY systems_report ON fees.reconciliation_issues FOR ALL TO acadex_app
    USING (identity.current_app_role() IN ('worker', 'gateway'))
    WITH CHECK (identity.current_app_role() IN ('worker', 'gateway'));

-- Reconciliation reads what it compares.
CREATE POLICY worker_reads ON fees.payment_attempts FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'worker');
CREATE POLICY worker_reads ON fees.payment_attempt_items FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'worker');
-- And the payments' refunds, which the gateway context needs too.
CREATE POLICY gateway_reads ON fees.refunds FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'gateway');
