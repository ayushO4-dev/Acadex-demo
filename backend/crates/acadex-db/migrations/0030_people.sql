CREATE TABLE people.students (
    user_id          uuid PRIMARY KEY REFERENCES identity.users ON DELETE CASCADE,
    program_id       smallint NOT NULL REFERENCES org.programs,
    roll_no          text NOT NULL UNIQUE,
    batch_year       smallint NOT NULL,  -- year of admission
    current_semester smallint NOT NULL CHECK (current_semester BETWEEN 1 AND 8)
);
CREATE INDEX ON people.students (program_id);

CREATE TABLE people.faculty (
    user_id     uuid PRIMARY KEY REFERENCES identity.users ON DELETE CASCADE,
    program_id  smallint NOT NULL REFERENCES org.programs,  -- home department
    designation text NOT NULL
);
