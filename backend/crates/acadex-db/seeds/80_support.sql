-- F5a: support tickets in every state, so the queue and the SLA clock have
-- something to show. Rutuja Shinde (support@northgate.example) works them.
--   Atharva: a resolved ticket about a receipt.
--   Sanika:  an open ticket about an attendance mark, unanswered for 7 hours
--          (its first-reply target is 8), and an old one already escalated.

BEGIN;

CREATE TEMP TABLE seed_tickets (n int, roll_no text, subject text, category text, priority text, status text,
                                age interval, assigned boolean, linked text, body text, reply text);
INSERT INTO seed_tickets VALUES
    (1, 'ECE24001', 'Fee receipt not generated', 'fees', 'high', 'resolved', interval '9 days', true, 'RCPT/2026-27/000004',
     'I paid my tuition on 24 Sept but the receipt hasn''t appeared under Receipts.',
     'The PDF was still being made when you looked; it''s there now. Sorry for the wait!'),
    (2, 'ECE24002', 'Marked absent on 29 Sept but I was in class', 'attendance', 'normal', 'open', interval '7 hours', false, 'EC502',
     'Dr. Saanvi Singh''s class, period 1. I sat in the second row and answered the roll call.', NULL),
    (3, 'ECE24002', 'Can''t download my hall ticket', 'examination', 'low', 'in_progress', interval '6 days', true, NULL,
     'The Hall Ticket page says my download is blocked.', NULL),
    (4, 'CSE24010', 'MOOC certificate rejected by mistake', 'academic', 'normal', 'waiting_on_requester', interval '2 days', true, 'NPTEL noc26-cs12',
     'My NPTEL certificate was rejected, but it has my name and roll number.',
     'Could you upload the certificate again as a PDF? The image is too blurry to read the score.'),
    (5, 'ECE24015', 'Wrong spelling of my name on the ID card', 'profile', 'normal', 'closed', interval '20 days', true, NULL,
     'My name is spelt "Rohan" but it should be "Rohaan".',
     'Please raise a name-change request from your Profile page; the registrar will approve it with your class 10 certificate.'),
    (6, 'CSE24021', 'Portal logs me out every few minutes', 'technical', 'high', 'open', interval '30 minutes', false, NULL,
     'On my phone the portal keeps signing me out while I fill the exam form.', NULL);

INSERT INTO support.tickets (requester_id, subject, category, priority, status, assignee_id, linked_resource,
                             created_at, first_response_due, resolve_due)
SELECT st.user_id, t.subject, t.category, t.priority, 'open',
       CASE WHEN t.assigned THEN agent.id END, t.linked, now() - t.age, now(), now()
FROM seed_tickets t
JOIN people.students st ON st.roll_no = t.roll_no
CROSS JOIN identity.users agent WHERE agent.email = 'support@northgate.example'
ORDER BY t.n;

-- Each ticket's messages, and its final status (the trigger stamps the dates).
INSERT INTO support.messages (ticket_id, author_id, body, created_at)
SELECT k.id, k.requester_id, t.body, k.created_at
FROM seed_tickets t JOIN support.tickets k ON k.subject = t.subject;
INSERT INTO support.messages (ticket_id, author_id, body, created_at)
SELECT k.id, agent.id, t.reply, k.created_at + interval '2 hours'
FROM seed_tickets t JOIN support.tickets k ON k.subject = t.subject
CROSS JOIN identity.users agent
WHERE agent.email = 'support@northgate.example' AND t.reply IS NOT NULL;
UPDATE support.tickets k SET first_responded_at = k.created_at + interval '2 hours'
FROM seed_tickets t WHERE k.subject = t.subject AND t.reply IS NOT NULL;
INSERT INTO support.messages (ticket_id, author_id, body, internal, created_at)
SELECT k.id, agent.id, 'Fees are clear; asked the exam cell why the download is blocked.', true, k.created_at + interval '1 day'
FROM support.tickets k CROSS JOIN identity.users agent
WHERE k.subject = 'Can''t download my hall ticket' AND agent.email = 'support@northgate.example';
UPDATE support.tickets k SET status = t.status FROM seed_tickets t WHERE k.subject = t.subject AND t.status <> 'open';

-- The hall ticket question has been open for six days: past its target, so
-- the SLA job escalated it.
UPDATE support.tickets SET escalation_level = 2, escalated_at = created_at + interval '5 days', priority = 'urgent'
WHERE subject = 'Can''t download my hall ticket';
INSERT INTO support.messages (ticket_id, author_id, body, internal, created_at)
SELECT id, NULL, 'Escalated: not resolved within the target time. Priority raised to urgent.', true, created_at + interval '5 days'
FROM support.tickets WHERE subject = 'Can''t download my hall ticket';

COMMIT;
