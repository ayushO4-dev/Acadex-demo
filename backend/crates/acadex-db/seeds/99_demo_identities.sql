-- The demo signs people in without Auth0: the website mints a token for the
-- subject `demo|<email>` (frontend/lib/session.ts). Link each account to its
-- subject, as a first Auth0 sign-in would.
INSERT INTO identity.user_identities (provider_subject, user_id)
SELECT 'demo|' || u.email, u.id
FROM identity.users u
WHERE NOT EXISTS (SELECT 1 FROM identity.user_identities i WHERE i.user_id = u.id)
ON CONFLICT DO NOTHING;
