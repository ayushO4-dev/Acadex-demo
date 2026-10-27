-- The demo's people have varied names: no first name or surname is used more
-- than 10 times, and no two people share a full name. Seeding fails here if a
-- change to an earlier file breaks that. Office and system accounts ("Exam
-- Cell Office", the break-glass account) and the platform operator don't count.

DO $$
DECLARE
    v_worst record;
BEGIN
    CREATE TEMP TABLE seed_people ON COMMIT DROP AS
    SELECT regexp_replace(display_name, '^(Dr\.|Prof\.) ', '') AS name
    FROM identity.users
    WHERE role <> 'platform_admin' AND display_name ~ '^((Dr\.|Prof\.) )?[A-Z][a-z]+ [A-Z][a-z]+$'
      AND display_name NOT IN ('Exam Cell Office', 'Break-glass Administrator');

    SELECT part, value, uses INTO v_worst FROM (
        SELECT 'first name' AS part, split_part(name, ' ', 1) AS value, count(*) AS uses FROM seed_people GROUP BY 2
        UNION ALL
        SELECT 'surname', split_part(name, ' ', 2), count(*) FROM seed_people GROUP BY 2
        UNION ALL
        SELECT 'full name', name, count(*) FROM seed_people GROUP BY 2 HAVING count(*) > 1
    ) t
    WHERE uses > CASE part WHEN 'full name' THEN 1 ELSE 10 END
    ORDER BY uses DESC LIMIT 1;
    IF FOUND THEN
        RAISE EXCEPTION 'demo names repeat too much: % "%" is used % times', v_worst.part, v_worst.value, v_worst.uses;
    END IF;
END $$;
