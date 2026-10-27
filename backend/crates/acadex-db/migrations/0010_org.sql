CREATE TABLE org.institutions (
    id   smallint PRIMARY KEY,
    name text NOT NULL
);

CREATE TABLE org.programs (
    id             smallint PRIMARY KEY,  -- also the attendance partition key
    institution_id smallint NOT NULL REFERENCES org.institutions,
    code           text NOT NULL UNIQUE CHECK (code ~ '^[a-z0-9_]+$'),  -- used in partition names
    name           text NOT NULL
);
