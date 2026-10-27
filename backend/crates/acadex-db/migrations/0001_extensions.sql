-- Extensions used across schemas; both are available on Cloud SQL for PostgreSQL.
CREATE EXTENSION IF NOT EXISTS pgcrypto;  -- digest() for the audit hash chain
CREATE EXTENSION IF NOT EXISTS pg_trgm;   -- universal search (architecture.md §4.3)
