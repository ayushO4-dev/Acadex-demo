-- The vendor's own operators, separate from every university role (its own
-- migration: a new enum value can't be used in the transaction that adds it).
ALTER TYPE identity.user_role ADD VALUE IF NOT EXISTS 'platform_admin';
