-- The rest of the roles in architecture.md §7.3. On their own: a new enum value
-- can't be used in the transaction that adds it.
ALTER TYPE identity.user_role ADD VALUE IF NOT EXISTS 'hod';
ALTER TYPE identity.user_role ADD VALUE IF NOT EXISTS 'accounts';
ALTER TYPE identity.user_role ADD VALUE IF NOT EXISTS 'support_agent';
ALTER TYPE identity.user_role ADD VALUE IF NOT EXISTS 'admin';
ALTER TYPE identity.user_role ADD VALUE IF NOT EXISTS 'super_admin';
