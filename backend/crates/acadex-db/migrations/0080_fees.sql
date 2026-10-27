-- Fees (architecture.md §9.4). Money is integer paise throughout: no floats,
-- no rounding.

-- The student ledger. Every charge, concession, payment and refund is an
-- immutable row (the app role can't UPDATE or DELETE it); "pending" is always
-- derived from the rows, never stored, so a balance can't drift.
CREATE TYPE fees.entry_kind AS ENUM ('charge', 'concession', 'payment', 'refund');

CREATE TABLE fees.payment_attempts (
    id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    student_id       uuid NOT NULL REFERENCES people.students,
    -- Sent by the browser per Pay click: a retried or double-submitted request
    -- finds the same attempt instead of opening a second order.
    idempotency_key  text NOT NULL CHECK (length(idempotency_key) BETWEEN 8 AND 100),
    amount_paise     bigint NOT NULL CHECK (amount_paise > 0),
    gateway_order_id text UNIQUE,
    -- review: the gateway reported something that doesn't match the attempt.
    status           text NOT NULL DEFAULT 'created'
                     CHECK (status IN ('created', 'captured', 'failed', 'review')),
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    UNIQUE (student_id, idempotency_key)
);

-- The webhook is the source of truth for a payment; the browser's return to
-- the site is advisory only.
CREATE TABLE fees.payments (
    id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    attempt_id         uuid NOT NULL UNIQUE REFERENCES fees.payment_attempts,
    student_id         uuid NOT NULL REFERENCES people.students,
    gateway_payment_id text NOT NULL UNIQUE,
    amount_paise       bigint NOT NULL CHECK (amount_paise > 0),
    method             text,
    captured_at        timestamptz NOT NULL
);

CREATE TABLE fees.ledger (
    id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    student_id    uuid NOT NULL REFERENCES people.students,
    academic_year smallint NOT NULL,
    kind          fees.entry_kind NOT NULL,
    -- A charge says what it's for; every other entry settles one charge.
    particulars   text,
    charge_id     bigint REFERENCES fees.ledger,
    amount_paise  bigint NOT NULL CHECK (amount_paise > 0),
    payment_id    uuid REFERENCES fees.payments,
    due_on        date,
    remarks       text,
    created_at    timestamptz NOT NULL DEFAULT now(),
    CHECK ((kind = 'charge') = (charge_id IS NULL)),
    CHECK ((kind = 'charge') = (particulars IS NOT NULL)),
    CHECK ((kind IN ('payment', 'refund')) = (payment_id IS NOT NULL))
);
CREATE INDEX ON fees.ledger (student_id, academic_year);
CREATE INDEX ON fees.ledger (charge_id);

-- What an attempt pays for, fixed when it is created.
CREATE TABLE fees.payment_attempt_items (
    attempt_id   uuid NOT NULL REFERENCES fees.payment_attempts,
    charge_id    bigint NOT NULL REFERENCES fees.ledger,
    amount_paise bigint NOT NULL CHECK (amount_paise > 0),
    PRIMARY KEY (attempt_id, charge_id)
);

-- Receipt numbers are gapless per financial year (April–March), as auditors
-- require: allocated by locking this row inside the confirming transaction,
-- so a rollback gives the number back.
CREATE TABLE fees.receipt_seq (
    financial_year smallint PRIMARY KEY,  -- 2026 = FY 2026-27
    last_no        integer NOT NULL
);

CREATE TABLE fees.receipts (
    id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    receipt_no     text NOT NULL UNIQUE,  -- RCPT/2026-27/000042
    financial_year smallint NOT NULL,
    seq            integer NOT NULL,
    payment_id     uuid NOT NULL UNIQUE REFERENCES fees.payments,
    student_id     uuid NOT NULL REFERENCES people.students,
    amount_paise   bigint NOT NULL CHECK (amount_paise > 0),
    remarks        text NOT NULL,
    issued_at      timestamptz NOT NULL DEFAULT now(),
    -- Cloud Storage object name, set by the worker once the PDF exists.
    pdf_object     text,
    UNIQUE (financial_year, seq)
);
CREATE INDEX ON fees.receipts (student_id, issued_at DESC);

-- Every webhook event, once: a redelivered event is recognised by its id and
-- does nothing.
CREATE TABLE fees.gateway_events (
    event_id    text PRIMARY KEY,
    event_type  text NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now(),
    payload     jsonb NOT NULL
);
