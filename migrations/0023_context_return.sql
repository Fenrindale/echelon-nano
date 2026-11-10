ALTER TABLE context_requests DROP CONSTRAINT context_requests_status_check;
ALTER TABLE context_requests ADD CONSTRAINT context_requests_status_check
    CHECK(status IN ('pending','delivered','returned'));

-- A return is a terminal curation disposition. Preserve the original criteria,
-- goal and request identity; replanning always creates a new request.
CREATE OR REPLACE FUNCTION guard_context_delivery() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.status <> 'pending'
       OR (to_jsonb(NEW) - ARRAY['status','delivery','pack_id'])
          IS DISTINCT FROM (to_jsonb(OLD) - ARRAY['status','delivery','pack_id'])
       OR NEW.delivery IS NULL
       OR NOT COALESCE(((NEW.status='delivered' AND NEW.pack_id IS NOT NULL)
           OR (NEW.status='returned' AND NEW.pack_id IS NULL
               AND NEW.delivery->>'disposition'='return_to_handler'
               AND jsonb_typeof(NEW.delivery->'reason')='string'
               AND NEW.delivery->>'request_id'=NEW.id::text)), false) THEN
        RAISE EXCEPTION 'context request is immutable and delivery is final';
    END IF;
    RETURN NEW;
END;
$$;
