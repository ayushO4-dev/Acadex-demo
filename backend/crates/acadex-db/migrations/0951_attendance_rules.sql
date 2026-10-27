-- Attendance follows the calendar and settles: no marking on a holiday or a
-- class suspension, and after this many days a teacher can't change a
-- session's marks any more (0: no limit). Later changes go through a
-- student's correction request, which the HOD decides.
ALTER TABLE org.institution_settings
    ADD COLUMN attendance_lock_days smallint NOT NULL DEFAULT 7 CHECK (attendance_lock_days BETWEEN 0 AND 180);
GRANT SELECT (attendance_lock_days), UPDATE (attendance_lock_days) ON org.institution_settings TO acadex_app;
