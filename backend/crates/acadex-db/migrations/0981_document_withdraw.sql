-- A student can remove a document of theirs that hasn't been verified (a
-- wrong file, a blurry scan), unless a change request offered it as
-- evidence. Verified documents are the registrar's record and stay.
CREATE FUNCTION profile.remove_my_document(p_id uuid) RETURNS text
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_doc profile.documents;
BEGIN
    SELECT * INTO v_doc FROM profile.documents WHERE id = p_id AND student_id = identity.current_user_id() FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'no such document' USING ERRCODE = 'no_data_found';
    END IF;
    IF v_doc.verification = 'verified' THEN
        RAISE EXCEPTION 'a verified document stays on record' USING ERRCODE = 'check_violation';
    END IF;
    IF EXISTS (SELECT 1 FROM profile.edit_requests WHERE document_id = p_id) THEN
        RAISE EXCEPTION 'a change request offers it as evidence' USING ERRCODE = 'check_violation';
    END IF;
    DELETE FROM profile.documents WHERE id = p_id;
    RETURN v_doc.object;
END $$;
REVOKE EXECUTE ON FUNCTION profile.remove_my_document(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION profile.remove_my_document(uuid) TO acadex_app;
