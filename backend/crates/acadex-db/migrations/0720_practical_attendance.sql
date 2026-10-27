-- Theory and practical attendance, logged separately. A subject may have a
-- practical component alongside its lectures; a lab subject is all practical.
-- Each component is held to the attendance threshold on its own.

ALTER TABLE academic.subjects ADD COLUMN has_practical boolean NOT NULL DEFAULT false;

ALTER TABLE academic.attendance
    ADD COLUMN session_kind text NOT NULL DEFAULT 'theory' CHECK (session_kind IN ('theory', 'practical'));

-- Lab subjects' sessions so far were practicals.
UPDATE academic.attendance a SET session_kind = 'practical'
FROM academic.offerings o JOIN academic.subjects s ON s.id = o.subject_id
WHERE o.id = a.offering_id AND s.kind = 'lab';

GRANT SELECT (has_practical) ON academic.subjects TO acadex_app;
